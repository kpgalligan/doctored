"""Author and reopen the bounded PSD corpus inside GIMP's Python batch process.

The PSD writer, layer reader and both reference renders belong to GIMP. This
script does not write PSD bytes or use Doctored as an oracle. Run with the
private launcher described in the adjacent provenance document.
"""
import hashlib
import json
import os
from pathlib import Path

from gi.repository import Gegl, Gimp, Gio

OUTPUT = Path(os.environ['DR_PSD_EXCHANGE_OUTPUT'])
OUTPUT.mkdir(parents=True, exist_ok=False)


def rectangle(width, height):
    return Gegl.Rectangle.new(0, 0, width, height)


def samples(drawable, scalar=False):
    width, height = drawable.get_width(), drawable.get_height()
    # No requested Babl format: retain the drawable's own profile encoding.
    # Requesting plain R'G'B'A u8 instead would implicitly convert P3 to sRGB.
    raw = list(drawable.get_buffer().get(rectangle(width, height), 1.0, None, Gegl.AbyssPolicy.NONE))
    if scalar:
        assert len(raw) == width * height
        return raw
    if drawable.get_image().get_base_type() == Gimp.ImageBaseType.GRAY:
        assert len(raw) == width * height * 2
        return [sample for i in range(0, len(raw), 2) for sample in (raw[i], raw[i], raw[i], raw[i + 1])]
    assert len(raw) == width * height * 4
    return raw


def write_samples(drawable, values, scalar=False):
    encoding = "Y u8" if scalar else "R'G'B'A u8"
    drawable.get_buffer().set(rectangle(drawable.get_width(), drawable.get_height()), encoding, bytes(values))
    drawable.update(0, 0, drawable.get_width(), drawable.get_height())


def procedure(name, **properties):
    proc = Gimp.get_pdb().lookup_procedure(name)
    config = proc.create_config()
    for key, value in properties.items():
        config.set_property(key.replace('_', '-'), value)
    result = proc.run(config)
    assert result.index(0) == Gimp.PDBStatusType.SUCCESS, (name, str(result.index(0)))
    return result


def export(image, path, psd=False, profile=True):
    props = dict(run_mode=Gimp.RunMode.NONINTERACTIVE, image=image,
                 file=Gio.File.new_for_path(str(path)), include_color_profile=profile,
                 include_exif=False, include_iptc=False, include_xmp=False, include_thumbnail=False)
    if not psd:
        props.update(save_transparent=True, time=False, format='rgba8')
    procedure('file-psd-export' if psd else 'file-png-export', **props)


def snapshot(image, profile_path):
    rows = []

    def walk(layers, depth):
        for layer in reversed(layers):
            group = layer.is_group()
            if group:
                walk(layer.get_children(), depth + 1)
            row = dict(name=layer.get_name(), kind='group' if group else 'raster', depth=depth,
                       size=[layer.get_width(), layer.get_height()], offset=list(layer.get_offsets()[1:]),
                       opacity=layer.get_opacity() / 100.0, visible=layer.get_visible(),
                       blend=layer.get_mode().value_nick, blend_space=layer.get_blend_space().value_nick,
                       composite_space=layer.get_composite_space().value_nick,
                       composite_mode=layer.get_composite_mode().value_nick,
                       open=layer.get_expanded() if group else None,
                       rgba=None if group else samples(layer), mask=None)
            mask = layer.get_mask()
            if mask:
                row['mask'] = dict(size=[mask.get_width(), mask.get_height()],
                                   offset=list(mask.get_offsets()[1:]), coverage=samples(mask, True),
                                   enabled=layer.get_apply_mask())
            rows.append(row)

    walk(image.get_layers(), 0)
    # GIMP canonicalizes its builtin sRGB assignment to an implicit profile,
    # but its exporter embeds that effective profile when requested.
    # PSD export writes the effective profile even when its generic option
    # says not to. Capture actual app state; the gray case records that refusal.
    profile = image.get_effective_color_profile()
    profile_bytes = bytes(profile.get_icc_profile()) if profile else b''
    if profile_bytes:
        profile_path.write_bytes(profile_bytes)
    return dict(canvas=[image.get_width(), image.get_height()],
                color_mode=image.get_base_type().value_nick, precision=image.get_precision().value_nick,
                resolution=list(image.get_resolution()[1:]), layers=rows,
                saved_channels=[dict(name=channel.get_name(), size=[channel.get_width(), channel.get_height()],
                                     coverage=samples(channel, True)) for channel in image.get_channels()],
                profile=profile_path.name if profile_bytes else None,
                profile_sha256=hashlib.sha256(profile_bytes).hexdigest() if profile_bytes else None)


def raster(image, name, values, size=(4, 3), offset=(0, 0), opacity=100, mode=None, parent=None):
    gray = image.get_base_type() == Gimp.ImageBaseType.GRAY
    layer = Gimp.Layer.new(image, name, *size, Gimp.ImageType.GRAYA_IMAGE if gray else Gimp.ImageType.RGBA_IMAGE,
                           opacity, mode or Gimp.LayerMode.NORMAL)
    image.insert_layer(layer, parent, 0)
    layer.set_blend_space(Gimp.LayerColorSpace.RGB_NON_LINEAR)
    layer.set_composite_space(Gimp.LayerColorSpace.RGB_NON_LINEAR)
    layer.set_composite_mode(Gimp.LayerCompositeMode.UNION)
    write_samples(layer, values)
    layer.set_offsets(*offset)
    return layer


def mask(layer, values, enabled=True):
    value = layer.create_mask(Gimp.AddMaskType.WHITE)
    layer.add_mask(value)
    write_samples(value, values, True)
    layer.set_apply_mask(enabled)


def make_image(gray=False, profile=None):
    image = Gimp.Image.new_with_precision(4, 3, Gimp.ImageBaseType.GRAY if gray else Gimp.ImageBaseType.RGB,
                                         Gimp.Precision.U8_NON_LINEAR)
    image.set_resolution(300, 240)
    if profile:
        image.set_color_profile(Gimp.ColorProfile.new_from_file(Gio.File.new_for_path(profile)))
    elif not gray:
        image.set_color_profile(Gimp.ColorProfile.new_rgb_srgb())
    return image


def group(image, name, parent=None, opacity=100, mode=None, expanded=True):
    value = Gimp.GroupLayer.new(image, name)
    image.insert_layer(value, parent, 0)
    value.set_mode(mode or Gimp.LayerMode.NORMAL)
    value.set_opacity(opacity)
    value.set_blend_space(Gimp.LayerColorSpace.RGB_NON_LINEAR)
    value.set_composite_space(Gimp.LayerColorSpace.RGB_NON_LINEAR)
    value.set_composite_mode(Gimp.LayerCompositeMode.UNION)
    value.set_expanded(expanded)
    return value


def render(image, path, profile):
    # Render through the application's projection even for a single source;
    # exporting that source directly could expose its hidden RGB instead.
    rendered = Gimp.Image.new_with_precision(image.get_width(), image.get_height(),
                                             image.get_base_type(), image.get_precision())
    rendered.set_resolution(*image.get_resolution()[1:])
    rendered.set_color_profile(image.get_color_profile())
    layer = Gimp.Layer.new_from_visible(image, rendered, 'Independent layer projection')
    rendered.insert_layer(layer, None, 0)
    export(rendered, path, profile=profile)
    rendered.delete()


def record(name, image, profile=True):
    folder = OUTPUT / name
    folder.mkdir()
    authored = snapshot(image, folder / 'authored.icc')
    render(image, folder / 'authored.png', profile=profile)
    export(image, folder / 'source.psd', psd=True, profile=profile)
    image.delete()
    # GIMP's layered PSD loader is separate from its merged-only load procedure.
    loaded = procedure('file-psd-load', run_mode=Gimp.RunMode.NONINTERACTIVE,
                       file=Gio.File.new_for_path(str(folder / 'source.psd'))).index(1)
    reopened = snapshot(loaded, folder / 'reopened.icc')
    render(loaded, folder / 'reopened.png', profile=profile)
    loaded.delete()
    manifest = dict(schema=1, name=name, application={'name': 'GIMP', 'version': Gimp.version()},
                    author=authored, reopened=reopened,
                    profile_embedding_requested=profile,
                    opaque_backdrop=name != 'rgb_partial_alpha',
                    files={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(folder.iterdir()) if p.name != 'manifest.json'})
    if name == 'gray_mask':
        manifest['expected_refusal_contains'] = 'embedded ICC profile is unsupported or invalid'
    if name == 'rgb_saved_channel':
        manifest['expected_loss_contains'] = 'additional saved channels'
    (folder / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


def main():
    assert Gimp.version() == '3.2.6', 'This authoring receipt fixes the application version'
    coverage = [0, 32, 96, 255, 255, 192, 128, 64, 3, 73, 173, 253]
    for name, enabled in [('rgb_mask', True), ('rgb_mask_disabled', False)]:
        image = make_image()
        raster(image, 'Opaque backdrop', [43, 101, 169, 255] * 12)
        top = raster(image, 'Independent ordinary mask', [201, 79, 31, 191] * 12)
        mask(top, coverage, enabled)
        record(name, image)

    image = make_image()
    values = [211, 87, 13, 0, 63, 137, 201, 1, 101, 211, 67, 127, 197, 31, 173, 255] * 3
    raster(image, 'Straight alpha and hidden RGB', values)
    record('rgb_partial_alpha', image)

    image = make_image()
    raster(image, 'Opaque backdrop', [29, 107, 181, 255] * 12)
    values = [((i * 31 + 43) % 256) for i in range(6 * 4 * 4)]
    top = raster(image, 'Full signed storage', values, size=(6, 4), offset=(-2, -1))
    mask(top, [(i * 37) % 256 for i in range(24)])
    hidden = raster(image, 'Hidden wholly off canvas', [73, 139, 229, 0] * 4, size=(2, 2), offset=(8, -3))
    hidden.set_visible(False)
    record('rgb_offcanvas', image)

    for name, mode in [('rgb_group_isolated', Gimp.LayerMode.NORMAL),
                       ('rgb_group_pass', Gimp.LayerMode.PASS_THROUGH)]:
        image = make_image()
        raster(image, 'Opaque backdrop', [71, 127, 199, 255] * 12)
        folder = group(image, 'Folder properties', opacity=128 / 255 * 100, mode=mode, expanded=False)
        raster(image, 'Multiply child', [189, 93, 47, 193] * 12, parent=folder, mode=Gimp.LayerMode.MULTIPLY)
        raster(image, 'Screen child', [33, 157, 219, 137] * 12, parent=folder, mode=Gimp.LayerMode.SCREEN)
        mask(folder, coverage)
        record(name, image)

    image = make_image()
    raster(image, 'Opaque backdrop', [101, 53, 173, 255] * 12)
    outer = group(image, 'Outer folder', opacity=192 / 255 * 100)
    inner = group(image, 'Nested folder', parent=outer, opacity=128 / 255 * 100, expanded=False)
    raster(image, 'Nested source', [19, 211, 93, 127] * 12, parent=inner)
    invisible = group(image, 'Hidden folder')
    raster(image, 'Hidden child', [255, 0, 0, 255] * 12, parent=invisible)
    invisible.set_visible(False)
    record('rgb_nested_hidden', image)

    image = make_image(gray=True)
    raster(image, 'Gray backdrop', [73, 73, 73, 255] * 12)
    top = raster(image, 'Gray mask source', [191, 191, 191, 173] * 12)
    mask(top, coverage)
    record('gray_mask', image, profile=False)

    image = make_image(profile='/System/Library/ColorSync/Profiles/Display P3.icc')
    raster(image, 'Display P3 source', [211, 93, 17, 255, 39, 187, 233, 255] * 6)
    record('rgb_p3_ppi', image)

    image = make_image()
    raster(image, 'Saved-channel control', [43, 101, 169, 255] * 12)
    auxiliary = Gimp.Channel.new(image, 'Excluded auxiliary coverage', 4, 3, 100, Gegl.Color.new('red'))
    image.insert_channel(auxiliary, None, 0)
    write_samples(auxiliary, coverage, True)
    auxiliary.set_visible(False)
    record('rgb_saved_channel', image)


main()
