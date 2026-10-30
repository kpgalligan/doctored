"""Capture GIMP's merged-only interpretation of existing, unchanged PSDs.

Executed inside GIMP3.2.6 Python batch. No PSD encoder or white-matte arithmetic
appears here: the independent application reads its actual saved composite.
"""
import hashlib
import json
import os
from pathlib import Path

from gi.repository import Gegl, Gimp, Gio

SOURCE = Path(os.environ['DR_PSD_MERGED_SOURCE'])
OUTPUT = Path(os.environ['DR_PSD_MERGED_OUTPUT'])
OUTPUT.mkdir(parents=True, exist_ok=False)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def procedure(name, **properties):
    proc = Gimp.get_pdb().lookup_procedure(name)
    config = proc.create_config()
    for key, value in properties.items():
        config.set_property(key.replace('_', '-'), value)
    result = proc.run(config)
    assert result.index(0) == Gimp.PDBStatusType.SUCCESS, (name, str(result.index(0)))
    return result


def native_samples(drawable, scalar=False):
    width, height = drawable.get_width(), drawable.get_height()
    raw = list(drawable.get_buffer().get(Gegl.Rectangle.new(0, 0, width, height), 1.0,
                                         None, Gegl.AbyssPolicy.NONE))
    if scalar:
        assert len(raw) == width * height
        return raw
    gray = drawable.get_image().get_base_type() == Gimp.ImageBaseType.GRAY
    components = (1 if gray else 3) + int(drawable.has_alpha())
    assert len(raw) == width * height * components
    rgba = []
    for i in range(0, len(raw), components):
        rgba.extend([raw[i]] * 3 if gray else raw[i:i + 3])
        rgba.append(raw[i + components - 1] if drawable.has_alpha() else 255)
    return rgba


def capture(folder):
    source = folder / 'source.psd'
    before = sha(source)
    image = procedure('file-psd-load-merged', run_mode=Gimp.RunMode.NONINTERACTIVE,
                      file=Gio.File.new_for_path(str(source))).index(1)
    assert image.get_precision() == Gimp.Precision.U8_NON_LINEAR
    layers = image.get_layers()
    assert len(layers) == 1 and not layers[0].is_group(), 'The actual merged loader must supply one raster'
    layer = layers[0]
    target = OUTPUT / folder.name
    target.mkdir()
    profile = image.get_effective_color_profile()
    profile_bytes = bytes(profile.get_icc_profile())
    original = json.loads((folder / 'manifest.json').read_text())
    original_profile = (folder / original['reopened']['profile']).read_bytes()
    original_sha = hashlib.sha256(original_profile).hexdigest()
    assert original_sha == original['reopened']['profile_sha256']
    timestamp_change = profile_bytes != original_profile
    assert len(profile_bytes) == len(original_profile)
    assert profile_bytes[:24] + profile_bytes[36:] == original_profile[:24] + original_profile[36:]
    if folder.name == 'rgb_p3_ppi':
        assert not timestamp_change
    (target / 'merged.icc').write_bytes(profile_bytes)
    observed = dict(canvas=[image.get_width(), image.get_height()],
                    precision=image.get_precision().value_nick, color_mode=image.get_base_type().value_nick,
                    resolution=list(image.get_resolution()[1:]), profile='merged.icc',
                    profile_sha256=hashlib.sha256(profile_bytes).hexdigest(),
                    layers=[dict(name=layer.get_name(), kind='raster', depth=0,
                                 offset=list(layer.get_offsets()[1:]), size=[layer.get_width(), layer.get_height()],
                                 rgba=native_samples(layer), opacity=layer.get_opacity() / 100,
                                 visible=layer.get_visible(), blend=layer.get_mode().value_nick,
                                 mask=None)],
                    saved_channels=[dict(name=c.get_name(), size=[c.get_width(), c.get_height()],
                                         coverage=native_samples(c, True)) for c in image.get_channels()])
    assert layer.get_mask() is None
    projected = Gimp.Image.new_with_precision(image.get_width(), image.get_height(),
                                              image.get_base_type(), image.get_precision())
    projected.set_color_profile(image.get_color_profile())
    projected.set_resolution(*image.get_resolution()[1:])
    projected.insert_layer(Gimp.Layer.new_from_visible(image, projected, 'Merged-only projection'), None, 0)
    procedure('file-png-export', run_mode=Gimp.RunMode.NONINTERACTIVE, image=projected,
              file=Gio.File.new_for_path(str(target / 'merged.png')),
              include_color_profile=True, include_exif=False, include_iptc=False,
              include_xmp=False, include_thumbnail=False, save_transparent=True,
              time=False, format='rgba8')
    projected.delete()
    image.delete()
    assert sha(source) == before
    manifest = dict(schema=1, name=folder.name, application={'name': 'GIMP', 'version': Gimp.version()},
                    loader='file-psd-load-merged', source_psd_sha256=before, merged=observed,
                    original_icc_sha256=original_sha,
                    icc_creation_time_only_difference=timestamp_change,
                    files={p.name: sha(p) for p in sorted(target.iterdir())})
    (target / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


assert Gimp.version() == '3.2.6'
for manifest in sorted(SOURCE.glob('*/manifest.json')):
    capture(manifest.parent)
