#ifndef BROKEN_PIPELINE_BROKEN_PIPELINE_C_H
#define BROKEN_PIPELINE_BROKEN_PIPELINE_C_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32) && defined(BP_C_ABI_BUILD_SHARED)
#define BP_C_ABI_API __declspec(dllexport)
#elif defined(_WIN32) && defined(BP_C_ABI_USE_SHARED)
#define BP_C_ABI_API __declspec(dllimport)
#else
#define BP_C_ABI_API
#endif

#define BP_ABI_VERSION_MAJOR 0u
#define BP_ABI_VERSION_MINOR 1u
#define BP_ABI_VERSION_PATCH 0u
#define BP_ABI_VERSION_HEX 0x00010000u
#define BP_ABI_VERSION_STRING "0.1.0-draft"
#define BP_ABI_DRAFT 1u

enum {
  BP_ABI_VERSION_FLAG_DRAFT = 1u << 0
};

typedef uint64_t bp_flags;
typedef uint64_t bp_task_index;

typedef struct bp_abi_version {
  uint16_t major;
  uint16_t minor;
  uint16_t patch;
  uint16_t flags;
} bp_abi_version;

typedef struct bp_string_view {
  const char* data;
  size_t size;
} bp_string_view;

typedef struct bp_bytes_view {
  const void* data;
  size_t size;
} bp_bytes_view;

typedef enum bp_owner {
  BP_OWNER_BORROWED = 0,
  BP_OWNER_HOST = 1,
  BP_OWNER_PROVIDER = 2,
  BP_OWNER_SHARED = 3
} bp_owner;

typedef struct bp_string {
  bp_string_view view;
  bp_owner owner;
  void* release_context;
  void (*release_fn)(void* release_context, const char* data, size_t size);
} bp_string;

typedef struct bp_buffer {
  bp_bytes_view view;
  bp_owner owner;
  void* release_context;
  void (*release_fn)(void* release_context, const void* data, size_t size);
} bp_buffer;

typedef enum bp_result_code {
  BP_RESULT_OK = 0,
  BP_RESULT_INVALID_ARGUMENT = 1,
  BP_RESULT_ABI_MISMATCH = 2,
  BP_RESULT_NOT_IMPLEMENTED = 3,
  BP_RESULT_HOST_ERROR = 4,
  BP_RESULT_PROVIDER_ERROR = 5,
  BP_RESULT_INVALID_STATE = 6
} bp_result_code;

typedef enum bp_task_state {
  BP_TASK_STATE_CONTINUE = 0,
  BP_TASK_STATE_BLOCKED = 1,
  BP_TASK_STATE_YIELD = 2,
  BP_TASK_STATE_FINISHED = 3,
  BP_TASK_STATE_CANCELLED = 4
} bp_task_state;

typedef enum bp_task_hint {
  BP_TASK_HINT_UNSPECIFIED = 0,
  BP_TASK_HINT_CPU = 1,
  BP_TASK_HINT_IO = 2
} bp_task_hint;

typedef struct bp_error {
  bp_result_code code;
  bp_string message;
} bp_error;

typedef struct bp_awaiter_iface {
  void* object;
  bp_result_code (*wait)(void* object, bp_error* out_error);
  void (*retain)(void* object);
  void (*release)(void* object);
} bp_awaiter_iface;

typedef struct bp_task_poll {
  bp_task_state state;
  bp_awaiter_iface awaiter;
} bp_task_poll;

struct bp_runtime_host_iface;
struct bp_build_host_iface;

typedef bp_result_code (*bp_task_fn)(
    const struct bp_runtime_host_iface* host,
    void* provider_context,
    bp_task_index task_index,
    bp_task_poll* out_poll,
    bp_error* out_error);

typedef bp_result_code (*bp_continuation_fn)(
    const struct bp_runtime_host_iface* host,
    void* provider_context,
    bp_task_poll* out_poll,
    bp_error* out_error);

typedef struct bp_task_descriptor {
  bp_string_view name;
  bp_task_hint hint;
  bp_flags flags;
  bp_task_fn run;
  void* provider_context;
} bp_task_descriptor;

typedef struct bp_continuation_descriptor {
  bp_string_view name;
  bp_task_hint hint;
  bp_flags flags;
  bp_continuation_fn run;
  void* provider_context;
} bp_continuation_descriptor;

typedef struct bp_task_group_descriptor {
  bp_string_view name;
  bp_flags flags;
  uint64_t task_count;
  bp_task_descriptor task;
  const bp_continuation_descriptor* continuation;
} bp_task_group_descriptor;

typedef struct bp_task_group_handle {
  void* object;
} bp_task_group_handle;

typedef struct bp_build_request {
  bp_string_view provider_api_name;
  uint64_t provider_api_version;
  bp_buffer payload;
  bp_flags flags;
} bp_build_request;

typedef struct bp_build_artifact {
  bp_string_view provider_api_name;
  uint64_t provider_api_version;
  bp_buffer executable;
  bp_flags runtime_flags;
} bp_build_artifact;

typedef struct bp_runtime_host_vtable {
  bp_result_code (*emit_diagnostic)(
      void* host_state,
      bp_string_view domain,
      bp_string_view message,
      bp_error* out_error);
  bp_result_code (*schedule_task_group)(
      void* host_state,
      const bp_task_group_descriptor* group,
      bp_task_group_handle* out_handle,
      bp_error* out_error);
  bp_result_code (*await_task_group)(
      void* host_state,
      bp_task_group_handle handle,
      bp_task_state* out_state,
      bp_error* out_error);
  void (*release_task_group_handle)(void* host_state, bp_task_group_handle handle);
} bp_runtime_host_vtable;

typedef struct bp_runtime_host_iface {
  void* host_state;
  const bp_runtime_host_vtable* vtable;
} bp_runtime_host_iface;

typedef struct bp_build_host_vtable {
  bp_result_code (*emit_diagnostic)(
      void* host_state,
      bp_string_view domain,
      bp_string_view message,
      bp_error* out_error);
} bp_build_host_vtable;

typedef struct bp_build_host_iface {
  void* host_state;
  const bp_build_host_vtable* vtable;
} bp_build_host_iface;

typedef struct bp_runtime_provider_vtable {
  bp_result_code (*bind_task_group)(
      void* provider_state,
      const bp_runtime_host_iface* host,
      const bp_build_artifact* artifact,
      bp_task_group_descriptor* out_group,
      bp_error* out_error);
  void (*release_task_group)(
      void* provider_state,
      bp_task_group_descriptor* group);
} bp_runtime_provider_vtable;

typedef struct bp_build_provider_vtable {
  bp_result_code (*build)(
      void* provider_state,
      const bp_build_host_iface* host,
      const bp_build_request* request,
      bp_build_artifact* out_artifact,
      bp_error* out_error);
  void (*release_artifact)(
      void* provider_state,
      bp_build_artifact* artifact);
} bp_build_provider_vtable;

typedef struct bp_provider_descriptor {
  bp_string_view provider_name;
  bp_string_view provider_api_name;
  bp_abi_version abi_version;
  uint64_t provider_api_version;
  bp_flags capabilities;
  void* provider_state;
  const bp_runtime_provider_vtable* runtime_vtable;
  const bp_build_provider_vtable* build_vtable;
} bp_provider_descriptor;

#define BP_QUERY_PROVIDER_SYMBOL "bp_query_provider_v0"

BP_C_ABI_API bp_result_code bp_query_provider_v0(
    const bp_abi_version* requested_abi,
    bp_provider_descriptor* out_provider,
    bp_error* out_error);

#ifdef __cplusplus
}
#endif

#endif
