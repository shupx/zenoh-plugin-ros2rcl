#include <rcl/rcl.h>
#include <rmw/rmw.h>
#include <rmw/serialized_message.h>
#include <rcutils/error_handling.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

typedef struct { rcl_context_t context; rcl_node_t node; } bridge_context;
typedef struct { void *library; const rosidl_message_type_support_t *support; } bridge_codec;
typedef struct {
  bridge_context *context;
  void *library;
  int server;
  rcl_service_t service;
  rcl_client_t client;
} bridge_endpoint;

const char *bridge_error(void) { return rcutils_get_error_string().str; }
void bridge_reset_error(void) { rcutils_reset_error(); }

bridge_context *bridge_context_new(size_t domain, const char *name) {
  bridge_context *b = calloc(1, sizeof(*b));
  if (!b) return NULL;
  b->context = rcl_get_zero_initialized_context();
  b->node = rcl_get_zero_initialized_node();
  rcl_init_options_t opts = rcl_get_zero_initialized_init_options();
  if (rcl_init_options_init(&opts, rcl_get_default_allocator()) != RCL_RET_OK) goto fail;
  if (rcl_init_options_set_domain_id(&opts, domain) != RCL_RET_OK) {
    rcl_ret_t ignored = rcl_init_options_fini(&opts); (void)ignored; goto fail;
  }
  rcl_ret_t ret = rcl_init(0, NULL, &opts, &b->context);
  rcl_ret_t ignored = rcl_init_options_fini(&opts); (void)ignored;
  if (ret != RCL_RET_OK) goto fail;
  rcl_node_options_t node_opts = rcl_node_get_default_options();
  node_opts.use_global_arguments = false;
  ret = rcl_parse_arguments(0, NULL, rcl_get_default_allocator(), &node_opts.arguments);
  if (ret == RCL_RET_OK) ret = rcl_node_init(&b->node, name, "/", &b->context, &node_opts);
  ignored = rcl_node_options_fini(&node_opts); (void)ignored;
  if (ret != RCL_RET_OK) {
    ignored = rcl_shutdown(&b->context); ignored = rcl_context_fini(&b->context);
    (void)ignored; goto fail;
  }
  return b;
fail:
  free(b); return NULL;
}
void bridge_context_free(bridge_context *b) {
  rcl_ret_t ignored = rcl_node_fini(&b->node);
  ignored = rcl_shutdown(&b->context); ignored = rcl_context_fini(&b->context);
  (void)ignored; free(b);
}

bridge_codec *bridge_codec_new(const char *package, const char *ns, const char *type) {
  char path[512], symbol[1024];
  snprintf(path, sizeof(path), "lib%s__rosidl_typesupport_c.so", package);
  void *lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
  if (!lib) return NULL;
  snprintf(symbol, sizeof(symbol), "rosidl_typesupport_c__get_message_type_support_handle__%s__%s__%s", package, ns, type);
  const rosidl_message_type_support_t *(*getter)(void) = dlsym(lib, symbol);
  if (!getter) { dlclose(lib); return NULL; }
  bridge_codec *c = calloc(1, sizeof(*c));
  if (!c) { dlclose(lib); return NULL; }
  c->library = lib; c->support = getter(); return c;
}
void bridge_codec_free(bridge_codec *c) { dlclose(c->library); free(c); }
int bridge_serialize(bridge_codec *c, const void *message, unsigned char **out, size_t *size) {
  rmw_serialized_message_t buf = rmw_get_zero_initialized_serialized_message();
  rcutils_allocator_t allocator = rcutils_get_default_allocator();
  if (rmw_serialized_message_init(&buf, 0, &allocator) != RMW_RET_OK) return -1;
  int ret = rmw_serialize(message, c->support, &buf);
  if (ret == RMW_RET_OK) {
    *out = malloc(buf.buffer_length);
    if (!*out) ret = RMW_RET_ERROR;
    else { *size = buf.buffer_length; memcpy(*out, buf.buffer, *size); }
  }
  rmw_ret_t ignored = rmw_serialized_message_fini(&buf); (void)ignored;
  return ret;
}
int bridge_deserialize(bridge_codec *c, const unsigned char *data, size_t size, void *message) {
  rmw_serialized_message_t buf = rmw_get_zero_initialized_serialized_message();
  buf.buffer = (unsigned char *)data; buf.buffer_length = size; buf.buffer_capacity = size;
  return rmw_deserialize(&buf, c->support, message);
}
void bridge_free(void *p) { free(p); }

bridge_endpoint *bridge_endpoint_new(bridge_context *context, const char *package,
                                     const char *type, const char *name, int server, size_t depth) {
  char path[512], symbol[1024];
  snprintf(path, sizeof(path), "lib%s__rosidl_typesupport_c.so", package);
  void *lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
  if (!lib) return NULL;
  snprintf(symbol, sizeof(symbol), "rosidl_typesupport_c__get_service_type_support_handle__%s__srv__%s", package, type);
  const rosidl_service_type_support_t *(*getter)(void) = dlsym(lib, symbol);
  if (!getter) { dlclose(lib); return NULL; }
  bridge_endpoint *e = calloc(1, sizeof(*e));
  if (!e) { dlclose(lib); return NULL; }
  e->context = context; e->library = lib; e->server = server;
  rcl_ret_t ret;
  if (server) {
    e->service = rcl_get_zero_initialized_service();
    rcl_service_options_t opts = rcl_service_get_default_options();
    opts.qos.depth = depth;
    ret = rcl_service_init(&e->service, &context->node, getter(), name, &opts);
  } else {
    e->client = rcl_get_zero_initialized_client();
    rcl_client_options_t opts = rcl_client_get_default_options();
    opts.qos.depth = depth;
    ret = rcl_client_init(&e->client, &context->node, getter(), name, &opts);
  }
  if (ret != RCL_RET_OK) { dlclose(lib); free(e); return NULL; }
  return e;
}
void bridge_endpoint_free(bridge_endpoint *e) {
  rcl_ret_t ignored;
  if (e->server) ignored = rcl_service_fini(&e->service, &e->context->node);
  else ignored = rcl_client_fini(&e->client, &e->context->node);
  (void)ignored; dlclose(e->library); free(e);
}
/* Headers are retained per request, so concurrent service replies cannot be mixed. */
int bridge_take_request(bridge_endpoint *e, void *message, void **header) {
  rmw_request_id_t *id = calloc(1, sizeof(*id));
  if (!id) return -1;
  rcl_ret_t ret = rcl_take_request(&e->service, id, message);
  if (ret == RCL_RET_SERVICE_TAKE_FAILED) { free(id); return 1; }
  if (ret != RCL_RET_OK) { free(id); return -1; }
  *header = id; return 0;
}
int bridge_send_response(bridge_endpoint *e, void *header, void *message) {
  return rcl_send_response(&e->service, header, message);
}
int bridge_send_request(bridge_endpoint *e, void *message, int64_t *sequence) {
  return rcl_send_request(&e->client, message, sequence);
}
int bridge_take_response(bridge_endpoint *e, void *message, int64_t *sequence) {
  rmw_request_id_t id;
  rcl_ret_t ret = rcl_take_response(&e->client, &id, message);
  if (ret == RCL_RET_CLIENT_TAKE_FAILED) return 1;
  if (ret != RCL_RET_OK) return -1;
  *sequence = id.sequence_number; return 0;
}
