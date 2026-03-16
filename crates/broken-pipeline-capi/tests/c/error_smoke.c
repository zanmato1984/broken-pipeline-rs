#include "broken_pipeline_rs.h"

#include <stdio.h>
#include <string.h>

static bp_rs_task_status_result error_callback(size_t task_id, void* user_data) {
  (void)task_id;
  (void)user_data;
  return (bp_rs_task_status_result){false, BP_RS_TASK_STATUS_CANCELLED, "boom"};
}

int main(void) {
  bp_rs_task_definition task = {"ErrorSmokeTask", error_callback, NULL, false};
  bp_rs_run_result result = bp_rs_run_sequential_task_group(&task, 1, NULL);
  if (result.ok) {
    fprintf(stderr, "expected error result\n");
    return 1;
  }
  if (result.error_message == NULL || strstr(result.error_message, "boom") == NULL) {
    fprintf(stderr, "unexpected error message: %s\n",
            result.error_message == NULL ? "<null>" : result.error_message);
    bp_rs_free_error_message(result.error_message);
    return 1;
  }
  bp_rs_free_error_message(result.error_message);
  return 0;
}
