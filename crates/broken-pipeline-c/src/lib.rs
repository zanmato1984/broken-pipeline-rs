#![allow(non_camel_case_types)]

//! Rust-port-specific task-group C API for Broken Pipeline.
//!
//! The canonical generic ABI draft is vendored under
//! `third_party/broken-pipeline-abi/`. This crate intentionally exposes a smaller
//! surface for driving Rust schedulers and does not claim full coverage of that draft.

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use arrow_schema::ArrowError;
use broken_pipeline::{Continuation, Task, TaskHint, TaskHintType, TaskStatus};
use broken_pipeline_schedule::{
    NaiveParallelScheduler, SequentialCoroScheduler, TaskGroup, Traits,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum bp_c_task_status_code {
    BP_C_TASK_STATUS_CONTINUE = 0,
    BP_C_TASK_STATUS_BLOCKED = 1,
    BP_C_TASK_STATUS_YIELD = 2,
    BP_C_TASK_STATUS_FINISHED = 3,
    BP_C_TASK_STATUS_CANCELLED = 4,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct bp_c_task_status_result {
    pub ok: bool,
    pub status: bp_c_task_status_code,
    pub error_message: *const c_char,
}

pub type bp_c_task_callback =
    Option<unsafe extern "C" fn(task_id: usize, user_data: *mut c_void) -> bp_c_task_status_result>;
pub type bp_c_continuation_callback =
    Option<unsafe extern "C" fn(user_data: *mut c_void) -> bp_c_task_status_result>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct bp_c_task_definition {
    pub name: *const c_char,
    pub callback: bp_c_task_callback,
    pub user_data: *mut c_void,
    pub io_hint: bool,
}

unsafe impl Send for bp_c_task_definition {}
unsafe impl Sync for bp_c_task_definition {}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct bp_c_continuation_definition {
    pub name: *const c_char,
    pub callback: bp_c_continuation_callback,
    pub user_data: *mut c_void,
    pub io_hint: bool,
}

unsafe impl Send for bp_c_continuation_definition {}
unsafe impl Sync for bp_c_continuation_definition {}

#[repr(C)]
pub struct bp_c_run_result {
    pub ok: bool,
    pub status: bp_c_task_status_code,
    pub error_message: *mut c_char,
}

/// Run a task group through the naive scheduler.
///
/// # Safety
/// `task` must point to a valid task definition for the duration of the call.
/// If `continuation` is non-null, it must point to a valid continuation definition.
/// Any callback-provided strings must remain valid for the duration of each callback.
#[no_mangle]
pub unsafe extern "C" fn bp_c_run_naive_task_group(
    task: *const bp_c_task_definition,
    num_tasks: usize,
    continuation: *const bp_c_continuation_definition,
) -> bp_c_run_result {
    run_with_naive(task, num_tasks, continuation)
}

/// Run a task group through the sequential coroutine-style scheduler.
///
/// # Safety
/// `task` must point to a valid task definition for the duration of the call.
/// If `continuation` is non-null, it must point to a valid continuation definition.
/// Any callback-provided strings must remain valid for the duration of each callback.
#[no_mangle]
pub unsafe extern "C" fn bp_c_run_sequential_task_group(
    task: *const bp_c_task_definition,
    num_tasks: usize,
    continuation: *const bp_c_continuation_definition,
) -> bp_c_run_result {
    run_with_sequential(task, num_tasks, continuation)
}

/// Free an error message allocated by this library.
///
/// # Safety
/// `message` must either be null or a pointer previously returned by this library via
/// `bp_c_run_naive_task_group` or `bp_c_run_sequential_task_group`.
#[no_mangle]
pub unsafe extern "C" fn bp_c_free_error_message(message: *mut c_char) {
    if !message.is_null() {
        let _ = CString::from_raw(message);
    }
}

unsafe fn run_with_naive(
    task: *const bp_c_task_definition,
    num_tasks: usize,
    continuation: *const bp_c_continuation_definition,
) -> bp_c_run_result {
    match build_group(task, num_tasks, continuation) {
        Ok(group) => {
            let scheduler = NaiveParallelScheduler::default();
            let handle = scheduler.schedule_task_group(group, scheduler.make_task_context(None));
            to_run_result(scheduler.wait_task_group(handle))
        }
        Err(error) => to_run_result(Err(error)),
    }
}

unsafe fn run_with_sequential(
    task: *const bp_c_task_definition,
    num_tasks: usize,
    continuation: *const bp_c_continuation_definition,
) -> bp_c_run_result {
    match build_group(task, num_tasks, continuation) {
        Ok(group) => {
            let scheduler = SequentialCoroScheduler::default();
            let handle = scheduler.schedule_task_group(group, scheduler.make_task_context(None));
            to_run_result(scheduler.wait_task_group(handle))
        }
        Err(error) => to_run_result(Err(error)),
    }
}

unsafe fn build_group(
    task: *const bp_c_task_definition,
    num_tasks: usize,
    continuation: *const bp_c_continuation_definition,
) -> Result<TaskGroup, ArrowError> {
    let task_def = task
        .as_ref()
        .ok_or_else(|| ArrowError::ComputeError("task definition must not be null".into()))?;
    let task = make_task(*task_def)?;
    if let Some(continuation_def) = continuation.as_ref().copied() {
        let continuation = make_continuation(continuation_def)?;
        Ok(TaskGroup::with_continuation(
            c_string(task_def.name, "Task")?,
            task,
            num_tasks,
            continuation,
        ))
    } else {
        Ok(TaskGroup::new(
            c_string(task_def.name, "Task")?,
            task,
            num_tasks,
        ))
    }
}

unsafe fn make_task(definition: bp_c_task_definition) -> Result<Task<Traits>, ArrowError> {
    let name = c_string(definition.name, "Task")?;
    let hint = if definition.io_hint {
        TaskHint {
            kind: TaskHintType::Io,
        }
    } else {
        TaskHint {
            kind: TaskHintType::Cpu,
        }
    };

    Ok(Task::with_hint(
        name,
        move |_, task_id| invoke_task(definition, task_id),
        hint,
    ))
}

unsafe fn make_continuation(
    definition: bp_c_continuation_definition,
) -> Result<Continuation<Traits>, ArrowError> {
    let name = c_string(definition.name, "Continuation")?;
    let hint = if definition.io_hint {
        TaskHint {
            kind: TaskHintType::Io,
        }
    } else {
        TaskHint {
            kind: TaskHintType::Cpu,
        }
    };

    Ok(Continuation::with_hint(
        name,
        move |_| invoke_continuation(definition),
        hint,
    ))
}

unsafe fn invoke_task(
    definition: bp_c_task_definition,
    task_id: usize,
) -> Result<TaskStatus, ArrowError> {
    let callback = definition
        .callback
        .ok_or_else(|| ArrowError::ComputeError("task callback must not be null".into()))?;
    convert_status(callback(task_id, definition.user_data))
}

unsafe fn invoke_continuation(
    definition: bp_c_continuation_definition,
) -> Result<TaskStatus, ArrowError> {
    let callback = definition
        .callback
        .ok_or_else(|| ArrowError::ComputeError("continuation callback must not be null".into()))?;
    convert_status(callback(definition.user_data))
}

fn convert_status(result: bp_c_task_status_result) -> Result<TaskStatus, ArrowError> {
    if !result.ok {
        let message = if result.error_message.is_null() {
            "unknown C callback error".to_string()
        } else {
            unsafe { CStr::from_ptr(result.error_message) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(ArrowError::ComputeError(message));
    }

    match result.status {
        bp_c_task_status_code::BP_C_TASK_STATUS_CONTINUE => Ok(TaskStatus::Continue),
        bp_c_task_status_code::BP_C_TASK_STATUS_YIELD => Ok(TaskStatus::Yield),
        bp_c_task_status_code::BP_C_TASK_STATUS_FINISHED => Ok(TaskStatus::Finished),
        bp_c_task_status_code::BP_C_TASK_STATUS_CANCELLED => Ok(TaskStatus::Cancelled),
        bp_c_task_status_code::BP_C_TASK_STATUS_BLOCKED => Err(ArrowError::ComputeError(
            "C API callbacks cannot return BLOCKED; use the Rust API for blocked scheduling".into(),
        )),
    }
}

fn c_string(ptr: *const c_char, fallback: &str) -> Result<String, ArrowError> {
    if ptr.is_null() {
        return Ok(fallback.to_string());
    }
    Ok(unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned())
}

fn to_run_result(result: Result<TaskStatus, ArrowError>) -> bp_c_run_result {
    match result {
        Ok(status) => bp_c_run_result {
            ok: true,
            status: from_task_status(&status),
            error_message: ptr::null_mut(),
        },
        Err(error) => bp_c_run_result {
            ok: false,
            status: bp_c_task_status_code::BP_C_TASK_STATUS_CANCELLED,
            error_message: into_c_error(error.to_string()),
        },
    }
}

fn from_task_status(status: &TaskStatus) -> bp_c_task_status_code {
    match status {
        TaskStatus::Continue => bp_c_task_status_code::BP_C_TASK_STATUS_CONTINUE,
        TaskStatus::Blocked(_) => bp_c_task_status_code::BP_C_TASK_STATUS_BLOCKED,
        TaskStatus::Yield => bp_c_task_status_code::BP_C_TASK_STATUS_YIELD,
        TaskStatus::Finished => bp_c_task_status_code::BP_C_TASK_STATUS_FINISHED,
        TaskStatus::Cancelled => bp_c_task_status_code::BP_C_TASK_STATUS_CANCELLED,
    }
}

fn into_c_error(message: String) -> *mut c_char {
    let sanitized = message.replace('\0', " ");
    CString::new(sanitized).unwrap().into_raw()
}
