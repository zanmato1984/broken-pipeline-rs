#ifndef BROKEN_PIPELINE_RS_H
#define BROKEN_PIPELINE_RS_H

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum bp_rs_task_status_code {
  BP_RS_TASK_STATUS_CONTINUE = 0,
  BP_RS_TASK_STATUS_BLOCKED = 1,
  BP_RS_TASK_STATUS_YIELD = 2,
  BP_RS_TASK_STATUS_FINISHED = 3,
  BP_RS_TASK_STATUS_CANCELLED = 4
} bp_rs_task_status_code;

typedef struct bp_rs_task_status_result {
  bool ok;
  bp_rs_task_status_code status;
  const char* error_message;
} bp_rs_task_status_result;

typedef bp_rs_task_status_result (*bp_rs_task_callback)(size_t task_id, void* user_data);
typedef bp_rs_task_status_result (*bp_rs_continuation_callback)(void* user_data);

typedef struct bp_rs_task_definition {
  const char* name;
  bp_rs_task_callback callback;
  void* user_data;
  bool io_hint;
} bp_rs_task_definition;

typedef struct bp_rs_continuation_definition {
  const char* name;
  bp_rs_continuation_callback callback;
  void* user_data;
  bool io_hint;
} bp_rs_continuation_definition;

typedef struct bp_rs_run_result {
  bool ok;
  bp_rs_task_status_code status;
  char* error_message;
} bp_rs_run_result;

bp_rs_run_result bp_rs_run_naive_task_group(
    const bp_rs_task_definition* task,
    size_t num_tasks,
    const bp_rs_continuation_definition* continuation);

bp_rs_run_result bp_rs_run_sequential_task_group(
    const bp_rs_task_definition* task,
    size_t num_tasks,
    const bp_rs_continuation_definition* continuation);

void bp_rs_free_error_message(char* message);

#ifdef __cplusplus
}
#endif

#endif
