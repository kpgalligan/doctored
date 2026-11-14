#!/usr/bin/env python3
"""Check core/CLAUDE.md's "Layers" rule for the kernel modules. A step of `make lint`.

Standard library only. It reads source text, nothing else:

    python3 scripts/check_layers.py

A kernel module (`KERNEL_MODULES`, file names under `core/src`) holds a
feature's pixel math as free functions, so it must not name the document or
hub types (`FORBIDDEN`, matched as whole words). Comments, string literals
and char literals are blanked before the scan; lifetimes and loop labels stay
code. It fails on each line of a kernel module that names one of them and on
a listed module that does not exist. One line per failure on stdout, then a
summary line. Exit 0 when every check holds, 1 otherwise.

A new kernel module is added to `KERNEL_MODULES`.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

KERNEL_MODULES: tuple[str, ...] = (
    "adjust_bw8.rs",
    "adjust_color.rs",
    "align_distortion.rs",
    "align_distortion_match.rs",
    "align_distortion_pair.rs",
    "align_projection.rs",
    "align_vignette.rs",
    "adjust_color_balance.rs",
    "adjust_hue_saturation.rs",
    "adjust_levels.rs",
    "adjust_mix.rs",
    "adjust_replace_color.rs",
    "adjust_tone.rs",
    "adjust_white_balance.rs",
    "art_history_stroke.rs",
    "bayesian_color.rs",
    "bayesian_matting.rs",
    "bitmap_codec.rs",
    "bitmap_method.rs",
    "blend.rs",
    "blend_fill.rs",
    "blend_float.rs",
    "brush_overlay.rs",
    "clip_unit.rs",
    "closed_form_matting.rs",
    "cmyk_adjust.rs",
    "cmyk_adjust_ink.rs",
    "cmyk_artwork_render.rs",
    "cmyk_composite.rs",
    "cmyk_style_row.rs",
    "color_range_band.rs",
    "crop_background.rs",
    "depth_blur.rs",
    "display_encoding.rs",
    "duotone_ado.rs",
    "duotone_ink.rs",
    "edge_refine.rs",
    "edge_trace.rs",
    "edge_trace_corridor.rs",
    "equalize.rs",
    "exr_float.rs",
    "fade.rs",
    "feature_points.rs",
    "feature_register.rs",
    "file_info.rs",
    "file_info_legacy.rs",
    "fill_align_box.rs",
    "filter_kernels.rs",
    "float_primaries.rs",
    "focus_stack.rs",
    "foreground_protection.rs",
    "foreground_solve.rs",
    "gaussian_blur_filter.rs",
    "gaussian_plane.rs",
    "gen_fill.rs",
    "gen_variations.rs",
    "gradient_field.rs",
    "graph_cut.rs",
    "gray_encode.rs",
    "gray_working.rs",
    "hdr_local_adaptation.rs",
    "hdr_toning.rs",
    "hdr_transfer.rs",
    "heal_components.rs",
    "indexed_act.rs",
    "indexed_dither.rs",
    "indexed_edge.rs",
    "indexed_palette.rs",
    "indexed_psd_records.rs",
    "inpaint_em.rs",
    "inpaint_sample.rs",
    "jxl_encode.rs",
    "lab_blend.rs",
    "lab_encode.rs",
    "lab_select.rs",
    "lab_stack.rs",
    "lab_tone.rs",
    "landmark_region.rs",
    "lens_db.rs",
    "lens_interp.rs",
    "lens_model.rs",
    "liquify.rs",
    "local_detail.rs",
    "mask_invert.rs",
    "matting_laplacian.rs",
    "matting_samples.rs",
    "merge_hdr.rs",
    "model_mask.rs",
    "neural_colorize.rs",
    "neural_tiling.rs",
    "oklab.rs",
    "opacity_byte.rs",
    "ops.rs",
    "ops_filters.rs",
    "ops_photographic.rs",
    "ops_spatial.rs",
    "panorama_gain.rs",
    "panorama_seam.rs",
    "patchmatch.rs",
    "patchmatch_nnf.rs",
    "path_edit.rs",
    "path_fit.rs",
    "path_flatten.rs",
    "path_geometry.rs",
    "path_trace.rs",
    "pattern_define.rs",
    "pattern_tile.rs",
    "perspective_crop.rs",
    "perspective_mesh.rs",
    "perspective_plane.rs",
    "perspective_warp.rs",
    "plane_blur.rs",
    "plane_swatch.rs",
    "poisson.rs",
    "psd_comp_records.rs",
    "psd_framing.rs",
    "psd_path_records.rs",
    "psd_spot_records.rs",
    "puppet_arap.rs",
    "puppet_cholesky.rs",
    "puppet_mesh.rs",
    "puppet_render.rs",
    "radiance_hdr.rs",
    "raw_local_range.rs",
    "raw_local_tone.rs",
    "reference_point.rs",
    "remove_model.rs",
    "render_clouds.rs",
    "retouch_brush.rs",
    "retouch_encoding.rs",
    "retouch_smudge.rs",
    "sampled_matting.rs",
    "sampling.rs",
    "sampling_box.rs",
    "sampling_dehaze.rs",
    "sampling_denoise.rs",
    "sampling_develop_filter.rs",
    "sampling_disc.rs",
    "sampling_gaussian.rs",
    "sampling_gaussian_fast.rs",
    "sampling_lens.rs",
    "sampling_linear.rs",
    "sampling_motion.rs",
    "sampling_offset.rs",
    "sampling_presence.rs",
    "sampling_radial.rs",
    "sampling_rank.rs",
    "sampling_smart_sharpen.rs",
    "sampling_surface.rs",
    "sampling_walk.rs",
    "selection_quick.rs",
    "sky_decontaminate.rs",
    "sky_group.rs",
    "sky_harmonize.rs",
    "spot_heal_type.rs",
    "spot_overprint.rs",
    "stack_moments.rs",
    "stack_stat.rs",
    "style_aa_band.rs",
    "style_bevel_chisel.rs",
    "style_bevel_smooth.rs",
    "style_blend_if.rs",
    "style_contour.rs",
    "style_fx_bevel_emboss.rs",
    "style_fx_color_overlay.rs",
    "style_fx_drop_shadow.rs",
    "style_fx_gradient_overlay.rs",
    "style_fx_inner_glow.rs",
    "style_fx_inner_shadow.rs",
    "style_fx_outer_glow.rs",
    "style_fx_pattern_overlay.rs",
    "style_fx_satin.rs",
    "style_fx_stroke.rs",
    "style_glow_gradient.rs",
    "style_gradient.rs",
    "style_matte.rs",
    "style_noise.rs",
    "style_plane_ops.rs",
    "style_stroke_distance.rs",
    "style_stroke_fill.rs",
    "style_texture.rs",
    "thumbnail_reduce.rs",
    "tiff_directory.rs",
    "tiff_layer_block.rs",
    "tiff_layers.rs",
    "tiff_pages.rs",
    "tiff_spot_samples.rs",
    "tiled_soft_matting.rs",
    "trim_bounds.rs",
    "trimap_strokes.rs",
    "vector_mask_combine.rs",
    "vibrance_law.rs",
)
FORBIDDEN: tuple[str, ...] = ("DrDocument", "Layer", "PrecisionDocument", "PrecisionDocumentLayer",
                              "PrecisionImage", "Adjustment", "LayerStyle", "IccProfile", "Inventory")
FORBIDDEN_WORDS = re.compile(r"\b(?:" + "|".join(FORBIDDEN) + r")\b")
# `r"…"`, `r#"…"#`, `br…`, `cr…`: the prefix and its hashes, up to the opening quote.
RAW_STRING = re.compile(r'(?:br|cr|r)(#*)"')
# A quote, one character or one escape on the same line, then a quote. Every
# other `'` (a lifetime, a loop label) is code.
CHAR_LITERAL = re.compile(r"'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]{1,8}\}|[^\n])|[^\\\n'])'")


def code_lines(text: str) -> list[tuple[int, str]]:
    """(1-based line, line) for every line of `text`, with comments and literals blanked.

    Blanked: `//` comments (doc comments included), nested `/* */` comments,
    string literals (escapes, raw strings) and char literals. Blanking keeps
    every newline, so line numbers hold.
    """
    out = list(text)
    length = len(text)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if out[index] != "\n":
                out[index] = " "

    i = 0
    while i < length:
        char = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = length if end < 0 else end
            blank(i, end)
            i = end
            continue
        if text.startswith("/*", i):
            depth, end = 0, i
            while end < length:
                if text.startswith("/*", end):
                    depth, end = depth + 1, end + 2
                elif text.startswith("*/", end):
                    depth, end = depth - 1, end + 2
                    if depth == 0:
                        break
                else:
                    end += 1
            blank(i, end)
            i = end
            continue
        after_word = i > 0 and (text[i - 1].isalnum() or text[i - 1] == "_")
        raw = None if after_word else RAW_STRING.match(text, i)
        if raw:
            close = text.find('"' + raw.group(1), raw.end())
            end = length if close < 0 else close + 1 + len(raw.group(1))
            blank(i, end)
            i = end
            continue
        if char == '"':
            end = i + 1
            while end < length and text[end] != '"':
                end += 2 if text[end] == "\\" else 1
            end = min(end + 1, length)
            blank(i, end)
            i = end
            continue
        if char == "'":
            literal = CHAR_LITERAL.match(text, i)
            if literal:
                blank(i, literal.end())
                i = literal.end()
                continue
        i += 1
    return list(enumerate("".join(out).split("\n"), start=1))


def check(root: Path) -> list[str]:
    errors = []
    for name in KERNEL_MODULES:
        path = root / "core/src" / name
        if not path.is_file():
            errors.append(f"error: core/src/{name}: in KERNEL_MODULES but missing")
            continue
        for number, line in code_lines(path.read_text(encoding="utf-8")):
            named = dict.fromkeys(FORBIDDEN_WORDS.findall(line))
            for word in named:
                errors.append(f"error: core/src/{name}:{number}: names {word}; a kernel module must not name the "
                              'document or hub types (core/CLAUDE.md "Layers")')
    return errors


def main() -> int:
    errors = check(ROOT)
    for message in errors:
        print(message)
    if errors:
        print(f"check_layers: {len(errors)} failures")
        return 1
    print(f"check_layers: ok ({len(KERNEL_MODULES)} kernel modules)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
