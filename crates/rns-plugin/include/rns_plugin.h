#ifndef RNS_PLUGIN_H
#define RNS_PLUGIN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RNS_PLUGIN_ABI_MAJOR UINT32_C(1)
#define RNS_PLUGIN_ABI_MINOR UINT32_C(1)

/*
 * ABI major versions must match exactly. A minor version may only append new
 * fields to an existing structure; it may not reorder fields or change their
 * type or semantics. Each side reads only fields within struct_size and must
 * accept a newer minor version when the mandatory prefix is present.
 * No language exception or stack unwind may cross any ABI function or host
 * callback boundary. Rust and C++ implementations catch panic/exception inside
 * their wrappers. Memory corruption, process signals, and abort are not
 * isolated: an in-process plugin can terminate the host process.
 */

typedef int32_t rns_plugin_result_t;

#define RNS_PLUGIN_OK ((rns_plugin_result_t)0)
#define RNS_PLUGIN_ERROR ((rns_plugin_result_t)-1)

typedef int32_t rns_log_level_t;

#define RNS_LOG_ERROR ((rns_log_level_t)1)
#define RNS_LOG_WARN ((rns_log_level_t)2)
#define RNS_LOG_INFO ((rns_log_level_t)3)
#define RNS_LOG_DEBUG ((rns_log_level_t)4)
#define RNS_LOG_TRACE ((rns_log_level_t)5)

/*
 * Before returning RNS_PLUGIN_ERROR, a plugin logs one concrete cause at
 * RNS_LOG_ERROR. RNS_LOG_WARN is for recoverable problems, RNS_LOG_INFO for
 * infrequent state changes, and RNS_LOG_DEBUG/RNS_LOG_TRACE for diagnostics.
 * Per-packet success messages must not be logged at INFO.
 */

/* Optional fields are valid only when the matching bit is set. */
#define RNS_RX_METADATA_RSSI UINT32_C(1) << 0
#define RNS_RX_METADATA_SNR UINT32_C(1) << 1

typedef struct rns_rx_metadata {
    uint32_t valid_fields;
    /*
     * Whole decibels, rounded to the nearest integer by the plugin; ignored
     * unless the corresponding validity bit is set.
     */
    int16_t rssi_dbm;
    int16_t snr_db;
} rns_rx_metadata_t;

#if defined(__cplusplus)
static_assert(sizeof(rns_rx_metadata_t) == 8, "unexpected RX metadata layout");
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(rns_rx_metadata_t) == 8,
               "unexpected RX metadata layout");
#endif

typedef struct rns_host_api {
    uint32_t abi_major;
    uint32_t abi_minor;
    uint32_t struct_size;
    uint32_t reserved0;

    void *host_context;

    /*
     * All host callbacks are thread-safe, return quickly, and may be invoked
     * concurrently from any plugin thread. The host copies borrowed data and
     * enqueues work; it does not call back into the plugin from a callback.
     */

    /*
     * message is non-NULL, non-empty UTF-8, need not be NUL-terminated, and is
     * borrowed only for the duration of the call. The host copies it and adds
     * interface context; it does not duplicate a plugin's ERROR message.
     */
    void (*log)(void *host_context,
                rns_log_level_t level,
                const uint8_t *message,
                size_t message_len);

    /*
     * Reports the current physical bitrate in bits per second. Every plugin
     * must call it with a non-zero value during create(), before returning OK,
     * and call it again whenever the bitrate changes.
     */
    void (*set_bitrate)(void *host_context, uint64_t bitrate_bps);

    /*
     * Reports whether the instance can currently send and receive. Every
     * plugin must call it with 1 during create(), before returning OK, and call
     * it with 0 on loss of service and 1 again after recovery. No values other
     * than 0 and 1 are valid. On transition to offline, the host drops queued
     * TX packets and rejects new ones instead of replaying stale traffic after
     * recovery.
     */
    void (*set_online)(void *host_context, uint8_t online);

    /*
     * data and metadata are borrowed only for the duration of the call.
     * Metadata is optional: the host must accept metadata == NULL and/or
     * metadata_size == 0. The host must not read a metadata field unless it
     * fits completely within metadata_size and its validity bit is set. A
     * received packet always has data != NULL and data_len > 0; a plugin must
     * discard empty or hardware-invalid frames instead of passing them to the
     * host. Interface MTU limits only outbound send(); the host does not reject
     * RX frames by MTU and passes complete frames to normal transport parsing.
     */
    void (*rx_packet)(void *host_context,
                      const uint8_t *data,
                      size_t data_len,
                      const rns_rx_metadata_t *metadata,
                      size_t metadata_size);
} rns_host_api_t;

#define RNS_HOST_API_V1_0_SIZE                                             \
    (offsetof(rns_host_api_t, rx_packet) +                                 \
     sizeof(((rns_host_api_t *)0)->rx_packet))

typedef struct rns_plugin rns_plugin_t;

typedef struct rns_string {
    const uint8_t *data;
    size_t len;
} rns_string_t;

/* For static C string literals only; the terminating NUL is excluded. */
#define RNS_STRING_LITERAL(value)                                          \
    { (const uint8_t *)(value), sizeof(value) - 1 }

#define RNS_PLUGIN_INFO_NAME_MAX_SIZE ((size_t)128)
#define RNS_PLUGIN_INFO_VERSION_MAX_SIZE ((size_t)64)
#define RNS_PLUGIN_INFO_DESCRIPTION_MAX_SIZE ((size_t)4096)
#define RNS_PLUGIN_INFO_CONFIG_SCHEMA_MAX_SIZE ((size_t)65536)

typedef struct rns_plugin_info {
    /* Static, non-empty UTF-8 strings valid while the library is loaded. */
    rns_string_t name;
    rns_string_t version;
    rns_string_t description;
    /*
     * ABI 1.1: optional UTF-8 JSON Schema for the plugin-specific config
     * mapping. An empty string means that no web-configurable schema is
     * available. The string remains valid while the library is loaded.
     */
    rns_string_t config_schema_json;
} rns_plugin_info_t;

#define RNS_PLUGIN_INFO_V1_0_SIZE                                          \
    (offsetof(rns_plugin_info_t, description) +                            \
     sizeof(((rns_plugin_info_t *)0)->description))

#define RNS_PLUGIN_INFO_V1_1_SIZE                                          \
    (offsetof(rns_plugin_info_t, config_schema_json) +                     \
     sizeof(((rns_plugin_info_t *)0)->config_schema_json))

typedef struct rns_plugin_api {
    uint32_t abi_major;
    uint32_t abi_minor;
    uint32_t struct_size;
    uint32_t reserved0;

    /* Available without creating an instance; info is mandatory. */
    const rns_plugin_info_t *info;
    size_t info_size;

    /*
     * host and its host_context remain valid until destroy() returns.
     * config_yaml is a serialized UTF-8 YAML mapping containing only the
     * interface's plugin-specific `config:` subtree. It is not NUL-terminated
     * and is borrowed only during create(); NULL with config_len == 0 denotes
     * an empty mapping. Plugins reject unknown configuration keys.
     *
     * out_plugin is mandatory. The plugin sets *out_plugin to NULL before any
     * fallible work. On ERROR it releases all partially-created resources and
     * leaves *out_plugin NULL. Every successful call creates an independent,
     * fully initialized and running instance which may immediately send and
     * receive.
     *
     * All host callbacks and host_context are ready before create() is called.
     * A plugin performs all fallible initialization first, reports bitrate,
     * then calls set_online(1) as its final commit point. It must not call
     * rx_packet() before that point and must not perform ordinarily fallible
     * initialization after it. Once online is reported, RX callbacks are
     * allowed even before create() has physically returned; the host queues
     * such packets.
     */
    rns_plugin_result_t (*create)(const rns_host_api_t *host,
                                  const uint8_t *config_yaml,
                                  size_t config_len,
                                  rns_plugin_t **out_plugin);

    /*
     * send() is synchronous. The host calls it from a dedicated worker and
     * never calls it concurrently for the same instance. On success it returns
     * after physical transmission completes. The plugin must impose finite
     * timeouts on all hardware waits so that send() always returns. data_len
     * never exceeds the MTU configured and enforced by the host. The host
     * always passes data != NULL and 0 < data_len <= configured MTU.
     *
     * OK means the plugin considers physical transmission complete. ERROR
     * means success could not be confirmed; the packet might have been sent
     * fully, partially, or not at all. The host never retries the same packet
     * automatically. The plugin reports persistent service loss through
     * set_online().
     */
    rns_plugin_result_t (*send)(rns_plugin_t *plugin,
                                const uint8_t *data,
                                size_t data_len);

    /*
     * The host calls destroy() only after its TX worker has exited and no
     * send() call is active. destroy() first calls set_online(0), then stops
     * plugin threads, releases all instance resources, and waits for callbacks
     * already in progress. The host API and host_context remain valid until
     * destroy() returns. No callback may occur after destroy() returns.
     */
    void (*destroy)(rns_plugin_t *plugin);
} rns_plugin_api_t;

#define RNS_PLUGIN_API_V1_0_SIZE                                           \
    (offsetof(rns_plugin_api_t, destroy) +                                 \
     sizeof(((rns_plugin_api_t *)0)->destroy))

#if defined(_WIN32)
#define RNS_PLUGIN_EXPORT __declspec(dllexport)
#else
#define RNS_PLUGIN_EXPORT __attribute__((visibility("default")))
#endif

/*
 * The only symbol that a plugin library exports. It returns a valid static API
 * table or NULL and may be called repeatedly with the same result. No host
 * logger is available if this function returns NULL.
 *
 * Calls using different instance pointers may execute concurrently, including
 * create(), send(), and destroy(); plugins synchronize any library-global
 * state and keep instance lifecycles independent. For one instance, the host
 * runs at most one send() at a time, never overlaps send() with destroy(), and
 * calls destroy() exactly once. The instance pointer is invalid afterwards.
 */
RNS_PLUGIN_EXPORT const rns_plugin_api_t *rns_plugin_get_api(void);

#ifdef __cplusplus
}
#endif

#endif
