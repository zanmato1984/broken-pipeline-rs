use std::any::Any;
use std::fmt;
use std::sync::Arc;

use crate::types::{BpResult, PipelineTypes, TaskId};

pub trait Resumer: Any + Send + Sync {
    fn resume(&self);
    fn is_resumed(&self) -> bool;
    fn as_any(&self) -> &dyn Any;
}

pub trait Awaiter: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

pub type SharedResumer = Arc<dyn Resumer>;
pub type SharedAwaiter = Arc<dyn Awaiter>;

pub type ResumerFactory<T> = Arc<dyn Fn() -> BpResult<SharedResumer, T> + Send + Sync + 'static>;
pub type AwaiterFactory<T> =
    Arc<dyn Fn(Vec<SharedResumer>) -> BpResult<SharedAwaiter, T> + Send + Sync + 'static>;
/// The slice is borrowed only for the call; the returned awaiter is independently owned.
///
/// A borrowed factory cannot return an awaiter retaining its input slice:
/// ```compile_fail
/// use std::sync::Arc;
/// use broken_pipeline::{Awaiter, BorrowedAwaiterFactory, PipelineTypes, SharedResumer};
/// struct Types;
/// impl PipelineTypes for Types { type Batch = (); type Error = (); type Context = (); }
/// struct Kept(&'static [SharedResumer]);
/// impl Awaiter for Kept { fn as_any(&self) -> &dyn std::any::Any { self } }
/// let _: BorrowedAwaiterFactory<Types> = Arc::new(|slice| Ok(Arc::new(Kept(slice))));
/// ```
pub type BorrowedAwaiterFactory<T> =
    Arc<dyn Fn(&[SharedResumer]) -> BpResult<SharedAwaiter, T> + Send + Sync + 'static>;

enum ContextAwaiterFactory<T: PipelineTypes> {
    Owned(AwaiterFactory<T>),
    Borrowed(BorrowedAwaiterFactory<T>),
}

impl<T: PipelineTypes> Clone for ContextAwaiterFactory<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Owned(factory) => Self::Owned(Arc::clone(factory)),
            Self::Borrowed(factory) => Self::Borrowed(Arc::clone(factory)),
        }
    }
}

#[derive(Clone)]
pub struct TaskContext<T: PipelineTypes> {
    context: Arc<T::Context>,
    input_id: Option<usize>,
    resumer_factory: ResumerFactory<T>,
    awaiter_factory: ContextAwaiterFactory<T>,
}

impl<T: PipelineTypes> TaskContext<T> {
    pub fn new(
        context: T::Context,
        resumer_factory: ResumerFactory<T>,
        awaiter_factory: AwaiterFactory<T>,
    ) -> Self {
        Self {
            context: Arc::new(context),
            input_id: None,
            resumer_factory,
            awaiter_factory: ContextAwaiterFactory::Owned(awaiter_factory),
        }
    }

    pub fn from_shared_context(
        context: Arc<T::Context>,
        resumer_factory: ResumerFactory<T>,
        awaiter_factory: AwaiterFactory<T>,
    ) -> Self {
        Self {
            context,
            input_id: None,
            resumer_factory,
            awaiter_factory: ContextAwaiterFactory::Owned(awaiter_factory),
        }
    }

    pub fn from_shared_context_with_borrowed_awaiter(
        context: Arc<T::Context>,
        resumer_factory: ResumerFactory<T>,
        awaiter_factory: BorrowedAwaiterFactory<T>,
    ) -> Self {
        Self {
            context,
            input_id: None,
            resumer_factory,
            awaiter_factory: ContextAwaiterFactory::Borrowed(awaiter_factory),
        }
    }

    pub fn context(&self) -> &T::Context {
        self.context.as_ref()
    }

    pub fn shared_context(&self) -> Arc<T::Context> {
        Arc::clone(&self.context)
    }

    pub fn input_id(&self) -> Option<usize> {
        self.input_id
    }

    pub fn with_input_id(&self, input_id: usize) -> Self {
        Self {
            context: self.context.clone(),
            input_id: Some(input_id),
            resumer_factory: Arc::clone(&self.resumer_factory),
            awaiter_factory: self.awaiter_factory.clone(),
        }
    }

    pub fn make_resumer(&self) -> BpResult<SharedResumer, T> {
        (self.resumer_factory)()
    }

    pub fn make_awaiter(&self, resumers: Vec<SharedResumer>) -> BpResult<SharedAwaiter, T> {
        match &self.awaiter_factory {
            ContextAwaiterFactory::Owned(factory) => factory(resumers),
            ContextAwaiterFactory::Borrowed(factory) => factory(&resumers),
        }
    }

    /// Legacy owned factories require an allocating conversion to an owned Vec.
    pub fn make_awaiter_from_slice(
        &self,
        resumers: &[SharedResumer],
    ) -> BpResult<SharedAwaiter, T> {
        match &self.awaiter_factory {
            ContextAwaiterFactory::Owned(factory) => factory(resumers.to_vec()),
            ContextAwaiterFactory::Borrowed(factory) => factory(resumers),
        }
    }
}

#[derive(Clone)]
pub enum TaskStatus {
    Continue,
    Blocked(SharedAwaiter),
    Yield,
    Finished,
    Cancelled,
}

impl TaskStatus {
    pub fn is_continue(&self) -> bool {
        matches!(self, Self::Continue)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    pub fn is_yield(&self) -> bool {
        matches!(self, Self::Yield)
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished)
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub fn awaiter(&self) -> Option<&SharedAwaiter> {
        match self {
            Self::Blocked(awaiter) => Some(awaiter),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Continue => "CONTINUE",
            Self::Blocked(_) => "BLOCKED",
            Self::Yield => "YIELD",
            Self::Finished => "FINISHED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

impl fmt::Debug for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskHintType {
    Cpu,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskHint {
    pub kind: TaskHintType,
}

impl Default for TaskHint {
    fn default() -> Self {
        Self {
            kind: TaskHintType::Cpu,
        }
    }
}

type TaskFn<T> = dyn Fn(&TaskContext<T>, TaskId) -> BpResult<TaskStatus, T> + Send + Sync + 'static;
type ContinuationFn<T> = dyn Fn(&TaskContext<T>) -> BpResult<TaskStatus, T> + Send + Sync + 'static;

pub struct Task<T: PipelineTypes> {
    name: String,
    func: Arc<TaskFn<T>>,
    hint: TaskHint,
}

impl<T: PipelineTypes> Clone for Task<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            func: Arc::clone(&self.func),
            hint: self.hint,
        }
    }
}

impl<T: PipelineTypes> Task<T> {
    pub fn new<F>(name: impl Into<String>, func: F) -> Self
    where
        F: Fn(&TaskContext<T>, TaskId) -> BpResult<TaskStatus, T> + Send + Sync + 'static,
    {
        Self::with_hint(name, func, TaskHint::default())
    }

    pub fn with_hint<F>(name: impl Into<String>, func: F, hint: TaskHint) -> Self
    where
        F: Fn(&TaskContext<T>, TaskId) -> BpResult<TaskStatus, T> + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            func: Arc::new(func),
            hint,
        }
    }

    pub fn run(&self, ctx: &TaskContext<T>, task_id: TaskId) -> BpResult<TaskStatus, T> {
        (self.func)(ctx, task_id)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hint(&self) -> TaskHint {
        self.hint
    }
}

pub struct Continuation<T: PipelineTypes> {
    name: String,
    func: Arc<ContinuationFn<T>>,
    hint: TaskHint,
}

impl<T: PipelineTypes> Clone for Continuation<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            func: Arc::clone(&self.func),
            hint: self.hint,
        }
    }
}

impl<T: PipelineTypes> Continuation<T> {
    pub fn new<F>(name: impl Into<String>, func: F) -> Self
    where
        F: Fn(&TaskContext<T>) -> BpResult<TaskStatus, T> + Send + Sync + 'static,
    {
        Self::with_hint(name, func, TaskHint::default())
    }

    pub fn with_hint<F>(name: impl Into<String>, func: F, hint: TaskHint) -> Self
    where
        F: Fn(&TaskContext<T>) -> BpResult<TaskStatus, T> + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            func: Arc::new(func),
            hint,
        }
    }

    pub fn run(&self, ctx: &TaskContext<T>) -> BpResult<TaskStatus, T> {
        (self.func)(ctx)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hint(&self) -> TaskHint {
        self.hint
    }
}

#[derive(Clone)]
pub struct TaskGroup<T: PipelineTypes> {
    name: String,
    task: Task<T>,
    num_tasks: usize,
    continuation: Option<Continuation<T>>,
}

impl<T: PipelineTypes> TaskGroup<T> {
    pub fn new(name: impl Into<String>, task: Task<T>, num_tasks: usize) -> Self {
        Self {
            name: name.into(),
            task,
            num_tasks,
            continuation: None,
        }
    }

    pub fn with_continuation(
        name: impl Into<String>,
        task: Task<T>,
        num_tasks: usize,
        continuation: Continuation<T>,
    ) -> Self {
        Self {
            name: name.into(),
            task,
            num_tasks,
            continuation: Some(continuation),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn task(&self) -> &Task<T> {
        &self.task
    }

    pub fn num_tasks(&self) -> usize {
        self.num_tasks
    }

    pub fn continuation(&self) -> Option<&Continuation<T>> {
        self.continuation.as_ref()
    }
}
