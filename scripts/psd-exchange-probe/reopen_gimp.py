"""Reopen the frozen Doctored-written PSD corpus inside GIMP 3.2.6.

The reference direction is reversed from the 10A authoring probe: these PSDs
were written by Doctored and are never modified here. GIMP's own
`file-psd-load` and `file-psd-load-merged` are the independent readers, and its
`file-png-export` supplies both appearance references. No Doctored code,
decoder or expectation appears in this file. Run it with the private launcher
`run_reopen.py`; see the adjacent provenance document.
"""
import hashlib
import json
import os
from array import array
from pathlib import Path

from gi.repository import Gegl, Gimp, Gio

SOURCE = Path(os.environ['DR_PSD_REOPEN_SOURCE'])
OUTPUT = Path(os.environ['DR_PSD_REOPEN_OUTPUT'])
OUTPUT.mkdir(parents=True, exist_ok=False)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rectangle(width, height):
    return Gegl.Rectangle.new(0, 0, width, height)


def procedure(name, **properties):
    proc = Gimp.get_pdb().lookup_procedure(name)
    config = proc.create_config()
    for key, value in properties.items():
        config.set_property(key.replace('_', '-'), value)
    result = proc.run(config)
    assert result.index(0) == Gimp.PDBStatusType.SUCCESS, (name, str(result.index(0)))
    return result


def scalars(drawable):
    """The drawable's own buffer samples, without requesting a format.

    Requesting a plain 8-bit format here would both convert a P3 encoding to
    sRGB and quantize a 16-bit drawable; the 10A probe records the same reason.
    A `u16-non-linear` image stores two native-endian bytes per component.
    """
    width, height = drawable.get_width(), drawable.get_height()
    raw = bytes(drawable.get_buffer().get(rectangle(width, height), 1.0, None, Gegl.AbyssPolicy.NONE))
    if drawable.get_image().get_precision().value_nick == 'u16-non-linear':
        assert len(raw) % 2 == 0
        return list(array('H', raw))
    return list(raw)


def samples(drawable, scalar=False):
    width, height = drawable.get_width(), drawable.get_height()
    raw = scalars(drawable)
    if scalar:
        assert len(raw) == width * height
        return raw
    if drawable.get_image().get_base_type() == Gimp.ImageBaseType.GRAY:
        assert len(raw) == width * height * 2
        return [sample for i in range(0, len(raw), 2) for sample in (raw[i], raw[i], raw[i], raw[i + 1])]
    assert len(raw) == width * height * 4
    return raw


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
                       rgba=None if group else samples(layer), mask=None,
                       # What this application attached from the record's own
                       # `lfx2` body (PDB `gimp-drawable-get-filters`): the
                       # reading side's own report, never an expectation.
                       filters=[dict(name=f.get_name(), operation=f.get_operation_name(),
                                     visible=f.get_visible(), opacity=f.get_opacity(),
                                     blend=f.get_blend_mode().value_nick)
                                for f in layer.get_filters()])
            mask = layer.get_mask()
            if mask:
                row['mask'] = dict(size=[mask.get_width(), mask.get_height()],
                                   offset=list(mask.get_offsets()[1:]), coverage=samples(mask, True),
                                   enabled=layer.get_apply_mask())
            rows.append(row)

    walk(image.get_layers(), 0)
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


def render(image, path):
    """The application's own projection of what it loaded, as a PNG."""
    rendered = Gimp.Image.new_with_precision(image.get_width(), image.get_height(),
                                             image.get_base_type(), image.get_precision())
    rendered.set_resolution(*image.get_resolution()[1:])
    rendered.set_color_profile(image.get_color_profile())
    layer = Gimp.Layer.new_from_visible(image, rendered, 'Independent layer projection')
    rendered.insert_layer(layer, None, 0)
    depth16 = image.get_precision().value_nick == 'u16-non-linear'
    procedure('file-png-export', run_mode=Gimp.RunMode.NONINTERACTIVE, image=rendered,
              file=Gio.File.new_for_path(str(path)), include_color_profile=True,
              include_exif=False, include_iptc=False, include_xmp=False,
              include_thumbnail=False, save_transparent=True, time=False,
              format='rgba16' if depth16 else 'rgba8')
    rendered.delete()


def read(loader, source, target, stage):
    image = procedure(loader, run_mode=Gimp.RunMode.NONINTERACTIVE,
                      file=Gio.File.new_for_path(str(source))).index(1)
    observed = snapshot(image, target / f'{stage}.icc')
    render(image, target / f'{stage}.png')
    image.delete()
    return observed


def reopen(folder):
    source = folder / 'source.psd'
    before = sha(source)
    target = OUTPUT / folder.name
    target.mkdir()
    record = dict(schema=1, name=folder.name,
                  application={'name': 'GIMP', 'version': Gimp.version()},
                  source_psd_sha256=before, error=None, reopened=None, merged=None)
    try:
        # GIMP's layered PSD loader is separate from its merged-only procedure.
        record['reopened'] = read('file-psd-load', source, target, 'reopened')
        record['merged'] = read('file-psd-load-merged', source, target, 'merged')
    except BaseException as error:  # Recorded, never repaired: a failed read IS the receipt.
        record['error'] = f'{type(error).__name__}: {error}'
    assert sha(source) == before, 'the source PSD must be left untouched'
    record['files'] = {p.name: sha(p) for p in sorted(target.iterdir()) if p.name != 'gimp.json'}
    (target / 'gimp.json').write_text(json.dumps(record, indent=2) + '\n')


assert Gimp.version() == '3.2.6', 'this reopening receipt fixes the application version'
for manifest in sorted(SOURCE.glob('*/manifest.json')):
    reopen(manifest.parent)
