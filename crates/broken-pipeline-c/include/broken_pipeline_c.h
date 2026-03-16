#ifndef BROKEN_PIPELINE_C_H
#define BROKEN_PIPELINE_C_H

/*
 * This header describes the Rust port's current exported C ABI surface.
 *
 * The generic, implementation-neutral ABI/protocol draft is vendored separately under
 * `third_party/broken-pipeline-abi/` at the repository root. This header is intentionally
 * narrower today and should not be treated as the canonical shared ABI for all ports.
 */

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum bp_c_task_status_code {
  BP_C_TASK_STATUS_CONTINUE = 0,
  BP_C_TASK_STATUS_BLOCKED = 1,
  BP_C_TASK_STATUS_YIELD = 2,
  BP_C_TASK_STATUS_FINISHED = 3,
  BP_C_TASK_STATUS_CANCELLED = 4
} bp_c_task_status_code;

typedef struct bp_c_task_status_result {
  bool ok;
  bp_c_task_status_code status;
  const char* error_message;
} bp_c_task_status_result;

typedef bp_c_task_status_result (*bp_c_task_callback)(size_t task_id, void* user_data);
typedef bp_c_task_status_result (*bp_c_continuation_callback)(void* user_data);

typedef struct bp_c_task_definition {
  const char* name;
  bp_c_task_callback callback;
  void* user_data;
  bool io_hint;
} bp_c_task_definition;

typedef struct bp_c_continuation_definition {
  const char* name;
  bp_c_continuation_callback callback;
  void* user_data;
  bool io_hint;
} bp_c_continuation_definition;

typedef struct bp_c_run_result {
  bool ok;
  bp_c_task_status_code status;
  char* error_message;
} bp_c_run_result;

bp_c_run_result bp_c_run_naive_task_group(
    const bp_c_task_definition* task,
    size_t num_tasks,
    const bp_c_continuation_definition* continuation);

bp_c_run_result bp_c_run_sequential_task_group(
    const bp_c_task_definition* task,
    size_t num_tasks,
    const bp_c_continuation_definition* continuation);

void bp_c_free_error_message(char* message);

#ifdef __cplusplus
}
#endif

#endif
