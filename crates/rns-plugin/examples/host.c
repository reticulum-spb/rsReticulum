#include "rns_plugin.h"

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct host_context {
    const char *instance_name;
    const uint8_t *expected_data;
    size_t expected_len;
    size_t received_packets;
    uint64_t bitrate_bps;
    uint8_t online;
};

static void host_log(void *opaque,
                     rns_log_level_t level,
                     const uint8_t *message,
                     size_t message_len) {
    struct host_context *context = opaque;
    printf("[%s level=%d] %.*s\n", context->instance_name, (int)level,
           (int)message_len, (const char *)message);
}

static void host_set_bitrate(void *opaque, uint64_t bitrate_bps) {
    struct host_context *context = opaque;
    context->bitrate_bps = bitrate_bps;
}

static void host_set_online(void *opaque, uint8_t online) {
    struct host_context *context = opaque;
    context->online = online;
}

static void host_rx_packet(void *opaque,
                           const uint8_t *data,
                           size_t data_len,
                           const rns_rx_metadata_t *metadata,
                           size_t metadata_size) {
    struct host_context *context = opaque;
    uint32_t valid_fields = 0;

    if (metadata != NULL &&
        metadata_size >= offsetof(rns_rx_metadata_t, valid_fields) +
                             sizeof(metadata->valid_fields)) {
        valid_fields = metadata->valid_fields;
    }
    if ((valid_fields & RNS_RX_METADATA_RSSI) != 0 &&
        metadata_size >= offsetof(rns_rx_metadata_t, rssi_dbm) +
                             sizeof(metadata->rssi_dbm)) {
        printf("[%s] RSSI %d dBm\n", context->instance_name,
               (int)metadata->rssi_dbm);
    }
    if ((valid_fields & RNS_RX_METADATA_SNR) != 0 &&
        metadata_size >= offsetof(rns_rx_metadata_t, snr_db) +
                             sizeof(metadata->snr_db)) {
        printf("[%s] SNR %d dB\n", context->instance_name,
               (int)metadata->snr_db);
    }

    if (data_len != context->expected_len ||
        memcmp(data, context->expected_data, data_len) != 0) {
        fprintf(stderr, "%s: looped packet differs from TX packet\n",
                context->instance_name);
        exit(EXIT_FAILURE);
    }
    context->received_packets++;
}

static rns_plugin_t *create_instance(const rns_plugin_api_t *api,
                                     rns_host_api_t *host) {
    static const uint8_t config[] = "echo: true\n";
    rns_plugin_t *plugin = NULL;

    if (api->create(host, config, sizeof(config) - 1, &plugin) != RNS_PLUGIN_OK ||
        plugin == NULL) {
        fprintf(stderr, "cannot create %s\n", ((struct host_context *)
                    host->host_context)->instance_name);
        exit(EXIT_FAILURE);
    }
    if (((struct host_context *)host->host_context)->bitrate_bps == 0) {
        fprintf(stderr, "plugin did not report its bitrate during create\n");
        api->destroy(plugin);
        exit(EXIT_FAILURE);
    }
    if (((struct host_context *)host->host_context)->online != 1) {
        fprintf(stderr, "plugin did not report itself online during create\n");
        api->destroy(plugin);
        exit(EXIT_FAILURE);
    }
    return plugin;
}

static void test_optional_rx_metadata(void) {
    static const uint8_t packet[] = {0x01};
    struct host_context context = {
        "metadata-test", packet, sizeof(packet), 0, 0, 0,
    };
    rns_rx_metadata_t metadata = {
        .valid_fields = RNS_RX_METADATA_RSSI | RNS_RX_METADATA_SNR,
        .rssi_dbm = -97,
        .snr_db = 6,
    };

    host_rx_packet(&context, packet, sizeof(packet), NULL, 0);
    host_rx_packet(&context, packet, sizeof(packet), &metadata,
                   sizeof(metadata.valid_fields));
    metadata.valid_fields = UINT32_C(1) << 31;
    host_rx_packet(&context, packet, sizeof(packet), &metadata,
                   sizeof(metadata));
    metadata.valid_fields = RNS_RX_METADATA_RSSI | RNS_RX_METADATA_SNR;
    host_rx_packet(&context, packet, sizeof(packet), &metadata,
                   sizeof(metadata));

    if (context.received_packets != 4) {
        fputs("optional RX metadata tests failed\n", stderr);
        exit(EXIT_FAILURE);
    }
}

int main(int argc, char **argv) {
    typedef const rns_plugin_api_t *(*get_api_fn)(void);
    static const uint8_t first_packet[] = {0x01, 0x02, 0x03};
    static const uint8_t second_packet[] = {0xaa, 0xbb};
    struct host_context contexts[2] = {
        {"radio-a", first_packet, sizeof(first_packet), 0, 0, 0},
        {"radio-b", second_packet, sizeof(second_packet), 0, 0, 0},
    };
    rns_host_api_t hosts[2];
    rns_plugin_t *instances[2];
    const rns_plugin_api_t *api;
    get_api_fn get_api;
    void *library;
    size_t i;

    if (argc != 2) {
        fprintf(stderr, "usage: %s PLUGIN.so\n", argv[0]);
        return EXIT_FAILURE;
    }
    test_optional_rx_metadata();
    library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (library == NULL) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return EXIT_FAILURE;
    }
    *(void **)(&get_api) = dlsym(library, "rns_plugin_get_api");
    if (get_api == NULL) {
        fprintf(stderr, "dlsym: %s\n", dlerror());
        return EXIT_FAILURE;
    }
    api = get_api();
    if (api == NULL || api->abi_major != RNS_PLUGIN_ABI_MAJOR ||
        api->struct_size < RNS_PLUGIN_API_V1_0_SIZE || api->info == NULL ||
        api->info_size < RNS_PLUGIN_INFO_V1_0_SIZE ||
        api->info->name.data == NULL || api->info->name.len == 0 ||
        api->info->name.len > RNS_PLUGIN_INFO_NAME_MAX_SIZE ||
        api->info->version.data == NULL || api->info->version.len == 0 ||
        api->info->version.len > RNS_PLUGIN_INFO_VERSION_MAX_SIZE ||
        api->info->description.data == NULL || api->info->description.len == 0 ||
        api->info->description.len > RNS_PLUGIN_INFO_DESCRIPTION_MAX_SIZE ||
        api->create == NULL ||
        api->send == NULL || api->destroy == NULL) {
        fprintf(stderr, "incompatible plugin API\n");
        return EXIT_FAILURE;
    }
    printf("loaded %.*s %.*s: %.*s\n", (int)api->info->name.len,
           (const char *)api->info->name.data, (int)api->info->version.len,
           (const char *)api->info->version.data,
           (int)api->info->description.len,
           (const char *)api->info->description.data);

    for (i = 0; i < 2; i++) {
        hosts[i] = (rns_host_api_t){
            .abi_major = RNS_PLUGIN_ABI_MAJOR,
            .abi_minor = RNS_PLUGIN_ABI_MINOR,
            .struct_size = sizeof(rns_host_api_t),
            .host_context = &contexts[i],
            .log = host_log,
            .set_bitrate = host_set_bitrate,
            .set_online = host_set_online,
            .rx_packet = host_rx_packet,
        };
        instances[i] = create_instance(api, &hosts[i]);
        if (api->send(instances[i], contexts[i].expected_data,
                      contexts[i].expected_len) != RNS_PLUGIN_OK) {
            fprintf(stderr, "%s: send failed\n", contexts[i].instance_name);
            return EXIT_FAILURE;
        }
    }

    for (i = 0; i < 2; i++) {
        api->destroy(instances[i]);
        if (contexts[i].online != 0) {
            fprintf(stderr, "%s: plugin remained online after destroy\n",
                    contexts[i].instance_name);
            return EXIT_FAILURE;
        }
        if (contexts[i].received_packets != 1) {
            fprintf(stderr, "%s: expected one RX packet\n",
                    contexts[i].instance_name);
            return EXIT_FAILURE;
        }
    }

    /* Keep the library loaded until process exit; runtime unload is unsupported. */
    (void)library;
    puts("two independent plugin instances passed");
    return EXIT_SUCCESS;
}
