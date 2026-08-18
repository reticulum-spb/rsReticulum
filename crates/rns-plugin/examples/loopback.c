#include "rns_plugin.h"

#include <stdlib.h>
#include <string.h>

struct rns_plugin {
    const rns_host_api_t *host;
};

static void plugin_log(const rns_host_api_t *host,
                       rns_log_level_t level,
                       const char *message) {
    host->log(host->host_context, level, (const uint8_t *)message,
              strlen(message));
}

static rns_plugin_result_t loopback_create(const rns_host_api_t *host,
                                           const uint8_t *config_yaml,
                                           size_t config_len,
                                           rns_plugin_t **out_plugin) {
    rns_plugin_t *plugin;

    if (host == NULL || out_plugin == NULL) {
        return RNS_PLUGIN_ERROR;
    }
    *out_plugin = NULL;

    if (host->abi_major != RNS_PLUGIN_ABI_MAJOR ||
        host->struct_size < RNS_HOST_API_V1_0_SIZE || host->log == NULL ||
        host->set_bitrate == NULL || host->set_online == NULL ||
        host->rx_packet == NULL) {
        return RNS_PLUGIN_ERROR;
    }
    if (config_yaml == NULL && config_len != 0) {
        plugin_log(host, RNS_LOG_ERROR,
                   "loopback: config pointer is NULL but length is non-zero");
        return RNS_PLUGIN_ERROR;
    }

    plugin = calloc(1, sizeof(*plugin));
    if (plugin == NULL) {
        plugin_log(host, RNS_LOG_ERROR,
                   "loopback: cannot allocate plugin instance");
        return RNS_PLUGIN_ERROR;
    }

    plugin->host = host;
    *out_plugin = plugin;
    host->set_bitrate(host->host_context, UINT64_C(1000000000));
    host->set_online(host->host_context, 1);
    plugin_log(host, RNS_LOG_INFO, "loopback: instance created and running");
    return RNS_PLUGIN_OK;
}

static rns_plugin_result_t loopback_send(rns_plugin_t *plugin,
                                         const uint8_t *data,
                                         size_t data_len) {
    rns_rx_metadata_t rx_metadata;

    if (plugin == NULL || (data == NULL && data_len != 0)) {
        return RNS_PLUGIN_ERROR;
    }
    memset(&rx_metadata, 0, sizeof(rx_metadata));
    plugin->host->rx_packet(plugin->host->host_context, data, data_len,
                            &rx_metadata, sizeof(rx_metadata));

    return RNS_PLUGIN_OK;
}

static void loopback_destroy(rns_plugin_t *plugin) {
    if (plugin == NULL) {
        return;
    }
    plugin->host->set_online(plugin->host->host_context, 0);
    plugin_log(plugin->host, RNS_LOG_INFO, "loopback: instance destroyed");
    free(plugin);
}

static const rns_plugin_info_t LOOPBACK_INFO = {
    .name = RNS_STRING_LITERAL("Loopback"),
    .version = RNS_STRING_LITERAL("1.0.0"),
    .description =
        RNS_STRING_LITERAL("Returns every transmitted packet through the RX callback."),
};

static const rns_plugin_api_t LOOPBACK_API = {
    .abi_major = RNS_PLUGIN_ABI_MAJOR,
    .abi_minor = RNS_PLUGIN_ABI_MINOR,
    .struct_size = sizeof(rns_plugin_api_t),
    .reserved0 = 0,
    .info = &LOOPBACK_INFO,
    .info_size = sizeof(LOOPBACK_INFO),
    .create = loopback_create,
    .send = loopback_send,
    .destroy = loopback_destroy,
};

RNS_PLUGIN_EXPORT const rns_plugin_api_t *rns_plugin_get_api(void) {
    return &LOOPBACK_API;
}
