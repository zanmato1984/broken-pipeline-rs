#include "broken_pipeline_c.h"

#include <stdatomic.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  atomic_size_t finished;
  bool yielded[4];
} suite_state;

static bp_c_task_status_result task_callback(size_t task_id, void* user_data) {
  suite_state* state = (suite_state*)user_data;
  if (!state->yielded[task_id]) {
    state->yielded[task_id] = true;
    return (bp_c_task_status_result){true, BP_C_TASK_STATUS_YIELD, NULL};
  }

  atomic_fetch_add_explicit(&state->finished, 1, memory_order_relaxed);
  return (bp_c_task_status_result){true, BP_C_TASK_STATUS_FINISHED, NULL};
}

static bp_c_task_status_result continuation_callback(void* user_data) {
  suite_state* state = (suite_state*)user_data;
  if (atomic_load_explicit(&state->finished, memory_order_relaxed) != 4) {
    return (bp_c_task_status_result){false, BP_C_TASK_STATUS_CANCELLED,
                                      "continuation observed wrong task count"};
  }
  return (bp_c_task_status_result){true, BP_C_TASK_STATUS_FINISHED, NULL};
}

int main(void) {
  suite_state state = {0};
  bp_c_task_definition task = {"NaiveSmokeTask", task_callback, &state, false};
  bp_c_continuation_definition continuation = {
      "NaiveSmokeContinuation", continuation_callback, &state, false};

  bp_c_run_result result = bp_c_run_naive_task_group(&task, 4, &continuation);
  if (!result.ok) {
    fprintf(stderr, "naive task group failed: %s\n", result.error_message);
    bp_c_free_error_message(result.error_message);
    return 1;
  }
  if (result.status != BP_C_TASK_STATUS_FINISHED) {
    fprintf(stderr, "unexpected final status: %d\n", (int)result.status);
    return 1;
  }
  if (atomic_load_explicit(&state.finished, memory_order_relaxed) != 4) {
    fprintf(stderr, "unexpected finished count\n");
    return 1;
  }
  return 0;
}
