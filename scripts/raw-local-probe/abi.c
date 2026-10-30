#include "doctored_core.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static char *error = NULL;
static const uint64_t limit = UINT64_C(1024) * 1024 * 1024;
static void success(void) {
    if (error) { fprintf(stderr, "Unexpected FFI error: %s\n", error); abort(); }
}
static void refusal(void) {
    assert(error != NULL && strlen(error) > 0);
    dr_string_free(error); error = NULL;
}
static unsigned u16(const uint8_t *p) { return (unsigned)p[0] + ((unsigned)p[1] << 8); }
int main(void) {
    DrRawLocalMask *mask = dr_raw_local_mask_edit(NULL, 9, 7,
        "{\"op\":\"radial\",\"center\":[4.5,3.5],\"radius\":[2,2],\"hardness\":1}",
        limit, 0, NULL, &error);
    success(); assert(mask);
    size_t count = 0;
    const uint8_t *bytes = dr_raw_local_mask_bytes(mask, &count, &error);
    success(); assert(bytes && count == 16 + 9 * 7 * 2 && memcmp(bytes, "RZLM", 4) == 0);
    assert(bytes[4] == 1 && bytes[8] == 9 && bytes[12] == 7);
    for (unsigned y = 0; y < 7; ++y) for (unsigned x = 0; x < 9; ++x) {
        int dx = (int)x - 4, dy = (int)y - 3;
        assert(u16(bytes + 16 + 2 * (y * 9 + x)) == (dx * dx + dy * dy <= 4 ? 65535 : 0));
    }
    char *info = dr_raw_local_mask_info(mask, &error);
    success(); assert(info && strstr(info, "byte_length") && strstr(info, "142"));
    dr_string_free(info);
    uint8_t saved[142]; memcpy(saved, bytes, count);
    DrRawLocalMask *copy = dr_raw_local_mask_from_bytes(saved, count, limit, 0, NULL, &error);
    success(); assert(copy);
    uint64_t scratch = dr_raw_local_mask_preview_scratch(copy, 0, 0, 9, 7, 2, 0.5, 1, true, &error);
    success(); assert(scratch > 0 && scratch <= limit);
    DrJobControl *control = dr_job_control_create(); assert(control);
    assert(dr_job_control_set_scratch_limit(control, scratch));
    DrImage *preview = dr_raw_local_mask_preview(copy, NULL, NULL, 0, 0, 9, 7, 2, 0.5, 1, true, limit, 0, control, &error);
    success(); assert(preview && dr_image_width(preview) == 5 && dr_image_height(preview) == 4);
    assert(dr_image_pixels_rgba(preview));
    dr_image_free(preview); dr_job_control_free(control);
    dr_raw_local_mask_free(mask);
    bytes = dr_raw_local_mask_bytes(copy, &count, &error);
    success(); assert(count == sizeof(saved) && memcmp(bytes, saved, count) == 0);
    uint8_t output[142]; memset(output, 0xA6, sizeof(output));
    assert(!dr_raw_local_mask_copy(copy, output, sizeof(output), 0, 0, NULL, &error)); refusal();
    for (size_t i = 0; i < sizeof(output); ++i) assert(output[i] == 0xA6);
    assert(dr_raw_local_mask_copy(copy, output, sizeof(output), limit, 0, NULL, &error));
    success(); assert(memcmp(output, saved, sizeof(output)) == 0);
    const char *identity = "{\"version\":1,\"kind\":\"exif\",\"orientation\":1}";
    char *remap_plan = dr_raw_local_mask_remap_plan(copy, identity, 1, limit, 0, &error);
    success(); assert(remap_plan && strstr(remap_plan, "\"unchanged\":true"));
    dr_string_free(remap_plan);
    assert(!dr_raw_local_mask_remap(copy, identity, 1, limit, 0, NULL, &error)); success();
    remap_plan = dr_raw_local_mask_remap_plan(copy, identity, 0, limit, 0, &error);
    assert(!remap_plan); refusal();
    assert(!dr_raw_local_mask_remap_plan(NULL, identity, 1, limit, 0, &error)); refusal();
    assert(!dr_raw_local_mask_remap(NULL, identity, 1, limit, 0, NULL, &error)); refusal();
    assert(!dr_raw_source_geometry_plan(NULL, identity, limit, 0, &error)); refusal();
    assert(!dr_raw_source_geometry(NULL, identity, limit, 0, NULL, &error)); refusal();
    control = dr_job_control_create(); assert(control); dr_job_control_cancel(control);
    assert(!dr_raw_local_mask_preview(copy, NULL, NULL, 0, 0, 9, 7, 1, 1, 0, false, limit, 0, control, &error)); refusal();
    dr_job_control_free(control);
    assert(dr_raw_local_mask_edit(copy, 9, 7,
        "{\"op\":\"radial\",\"center\":[4.5,3.5],\"radius\":[2,2],\"hardness\":1}",
        limit, 0, NULL, &error) == NULL); success();
    assert(dr_raw_local_mask_preview_scratch(copy, 0, 0, 9, 7, 0, 1, 0, false, &error) == 0); refusal();
    count = 99; assert(!dr_raw_local_mask_bytes(NULL, &count, &error)); refusal(); assert(count == 0);
    assert(!dr_raw_local_mask_info(NULL, &error)); refusal();
    const char *invalid[] = {
        "{\"op\":\"empty\",\"unknown\":1}",
        "{\"op\":\"invert\",\"unknown\":1}",
        "{\"op\":\"empty\",\"radius\":[2,2]}",
        "{\"op\":\"invert\",\"center\":[4.5,3.5]}"
    };
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) {
        assert(!dr_raw_local_mask_edit(copy, 9, 7, invalid[i], limit, 0, NULL, &error)); refusal();
        bytes = dr_raw_local_mask_bytes(copy, &count, &error);
        success(); assert(count == sizeof(saved) && memcmp(bytes, saved, count) == 0);
    }
    dr_raw_local_mask_free(copy); dr_raw_local_mask_free(NULL);
    printf("PASS standalone C ABI: exact radial bytes, owned metadata, native roundtrip/lifetime, planned preview, additive geometry/remap plan/no-op/NULL, atomic refusal, cancellation and strict unknown-field contracts; scratch=%llu\n", (unsigned long long)scratch);
    return 0;
}
