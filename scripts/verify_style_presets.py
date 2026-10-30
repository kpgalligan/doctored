#!/usr/bin/env python3
"""Window ▸ Styles: the MCP drive of the seven style-preset
tools against an owned Debug app (`scripts/app_under_test.py`; a Release app
through DR_VERIFY_APP).

Items, FIXED before anything is measured; a failed item is the receipt and
nothing is widened afterwards. Every item prints PASS or FAIL.

- LIST     list_style_presets answers the twelve built-ins first, in
           "Default Styles", builtin true, file null; `groups` starts with it.
- SAVE     save_style_preset from a styled layer (drop shadow, Fill 50 %,
           layer Multiply 80 %): without blending the preset keeps the
           effects, resets fill to 1 and has no blending; with blending it
           keeps fill 0.5 and records {multiply, 0.8}.
- REPLACE  apply_style_preset "Soft Drop" on [1, 2] is ONE history step;
           layer 1 keeps its own Fill 50 % (a preset without blending), layer
           2 gets the style; one undo restores both.
- BLEND    the blending preset on [1, 2] where layer 1 already holds its
           style: both layers' mode and opacity change, layer 2's style
           changes, layer 1's does not; one history step.
- SKIP     [pixel, adjustment] applies to both: an RGB adjustment layer
           carries a style (only a CMYK one refuses), so nothing
           is skipped, and the adjustment layer holds the preset's effects.
- ADD      add true stacks a second drop shadow; a second identical replace
           answers `unchanged` with no history step.
- LIBRARY  rename, move to a group and back ("" ungroups), delete; a seeded
           malformed file is one `not_json` broken row cleared by `file`.
- REFUSE   "Default Styles" (any case) as a save group, a rename target group
           and an explicit import group, and "/" in a name, each refuse with
           one sentence; a built-in cannot be renamed or deleted.
- IMPORT   a hand-built `.asl` (the Photoshop-authored fixture is pending
           the confirmation take): the file's name is the group, a Stroke
           Emboss bevel is dropped and named ("Bevel & Emboss: …") while its
           stroke imports, a switched-off style is skipped, "/" in a name
           becomes "-", a blending style keeps {multiply, 0.7}; a second
           import suffixes " 2" and names its drops by that name; a file named "default styles.asl" lands in
           "Default Styles 2".
- EXPORT   export_style_presets of the built-in group and of the imported
           group writes a `.asl` naming nothing skipped; import_style_presets of
           each file into a fresh group restores every exported preset: the
           same names, styles and blending.
- DEPTHS   "Sticker Cut" applies at RGB16 (three effects back), "Soft Drop"
           at RGB32 and on a CMYK 8-bit document.
- RELAUNCH a second instance over the same folders lists the saved presets.
- ISOLATED ~/Library/Application Support/Doctored/Style Presets has the same
           existence and mtime before and after the run.

Export DR_BACKGROUND=1 before running. The instances are owned by this file
(`HostApp`: a scratch socket, a dummy key and a PRIVATE folder for every
preference library, DR_STYLE_PRESETS_DIR included) and end with it.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test  # noqa: E402
from support import HostApp  # noqa: E402

BUILTIN = 'Default Styles'
USER_LIBRARY = Path('~/Library/Application Support/Doctored/Style Presets').expanduser()
FAILURES = []


def check(label, condition, detail=''):
    print(f'{"PASS" if condition else "FAIL"} {label}' + (f' — {detail}' if detail else ''),
          flush=True)
    if not condition:
        FAILURES.append(label)


def library_state():
    if not USER_LIBRARY.exists():
        return None
    return USER_LIBRARY.stat().st_mtime_ns


# ------------------------------------------------------------ the .asl --

def uni(text):
    units = text.encode('utf-16-be')
    return struct.pack('>I', len(units) // 2 + 1) + units + b'\0\0'


def ident(key):
    return struct.pack('>I', 0 if len(key) == 4 else len(key)) + key


def desc(cls, items):
    return uni('') + ident(cls) + struct.pack('>I', len(items)) + b''.join(items)


def item(key, ostype, value):
    return ident(key) + ostype + value


def boolean(key, on):
    return item(key, b'bool', bytes([1 if on else 0]))


def untf(key, unit, value):
    return item(key, b'UntF', unit + struct.pack('>d', value))


def enum(key, type_id, enum_id):
    return item(key, b'enum', ident(type_id) + ident(enum_id))


def objc(key, cls, items):
    return item(key, b'Objc', desc(cls, items))


def text(key, value):
    return item(key, b'TEXT', uni(value))


def rgbc(key, r, g, b):
    return objc(key, b'RGBC', [item(b'Rd  ', b'doub', struct.pack('>d', r)),
                               item(b'Grn ', b'doub', struct.pack('>d', g)),
                               item(b'Bl  ', b'doub', struct.pack('>d', b))])


def drop_shadow():
    return objc(b'DrSh', b'DrSh', [
        boolean(b'enab', True), enum(b'Md  ', b'BlnM', b'Mltp'), rgbc(b'Clr ', 0, 0, 0),
        untf(b'Opct', b'#Prc', 60), boolean(b'uglg', False), untf(b'lagl', b'#Ang', 120),
        untf(b'Dstn', b'#Pxl', 5), untf(b'Ckmt', b'#Prc', 0), untf(b'blur', b'#Pxl', 7)])


def stroke():
    return objc(b'FrFX', b'FrFX', [
        boolean(b'enab', True), enum(b'Md  ', b'BlnM', b'Nrml'), untf(b'Opct', b'#Prc', 100),
        enum(b'Styl', b'FStl', b'OutF'), enum(b'PntT', b'FrFl', b'SClr'),
        untf(b'Sz  ', b'#Pxl', 2), rgbc(b'Clr ', 200, 30, 30)])


def stroke_emboss():
    return objc(b'ebbl', b'ebbl', [
        boolean(b'enab', True), enum(b'hglM', b'BlnM', b'Scrn'), rgbc(b'hglC', 255, 255, 255),
        untf(b'hglO', b'#Prc', 75), enum(b'sdwM', b'BlnM', b'Mltp'), rgbc(b'sdwC', 0, 0, 0),
        untf(b'sdwO', b'#Prc', 75), enum(b'bvlS', b'BESl', b'strokeEmboss'),
        untf(b'srgR', b'#Prc', 100), untf(b'blur', b'#Pxl', 4)])


def library(styles):
    out = struct.pack('>H', 2) + b'8BSL' + struct.pack('>H', 3) + struct.pack('>I', 0)
    out += struct.pack('>I', len(styles))
    for i, (name, items) in enumerate(styles):
        record = struct.pack('>I', 16) + desc(b'null', [text(b'Nm  ', name),
                                                         text(b'Idnt', f'id-{i}')])
        record += struct.pack('>I', 16) + desc(b'Styl', items)
        out += struct.pack('>I', len(record)) + record
        out += b'\0' * ((-len(out)) % 4)
    return out


def drive_library():
    lefx = lambda effects, on=True: objc(  # noqa: E731
        b'Lefx', b'Lefx', [untf(b'Scl ', b'#Prc', 100), boolean(b'masterFXSwitch', on)] + effects)
    return library([
        ('Imported Shadow', [lefx([drop_shadow()])]),
        ('Imported Emboss', [lefx([stroke_emboss(), stroke()])]),
        ('Off/Name', [lefx([drop_shadow()], on=False)]),
        ('Imported Blend', [objc(b'blendOptions', b'blendOptions', [
            enum(b'Md  ', b'BlnM', b'Mltp'), untf(b'Opct', b'#Prc', 70)]), lefx([drop_shadow()])]),
    ])


# ------------------------------------------------------------ helpers --

def new_document(app, **extra):
    created = app.call('new_document', width=96, height=64, **extra)
    return created['document']['id']


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def effects_of(row):
    style = row.get('style') or {}
    return [e['type'] for e in style.get('effects') or []]


def history(app, document):
    return len(app.call('list_history', document_id=document)['states'])


def presets(app, **arguments):
    return app.call('list_style_presets', **arguments)


def row_named(app, name, group=None):
    rows = [r for r in presets(app, include_style=True)['presets']
            if r['name'] == name and r['group'] == group]
    return rows[0] if len(rows) == 1 else None


def refusal(app, tool, **arguments):
    return app.client.call(tool, fail=True, **arguments)


# -------------------------------------------------------------- items --

def item_list(app):
    listed = presets(app)
    rows = listed['presets']
    builtins = rows[:12]
    check('LIST twelve built-ins first', len(rows) >= 12 and all(
        r['builtin'] and r['group'] == BUILTIN and r['file'] is None for r in builtins),
        json.dumps([r['name'] for r in builtins]))
    check('LIST groups start with Default Styles', listed['groups'][:1] == [BUILTIN],
          json.dumps(listed['groups']))


def styled_document(app):
    document = new_document(app)
    for color in ('#3070c0', '#c07030'):
        app.call('new_layer', document_id=document)
        top = len(layers(app, document)) - 1
        app.call('fill', document_id=document, layer=top, x=2, y=2, color=color)
    app.call('add_adjustment_layer', document_id=document, op='invert')
    app.call('set_layer_style', document_id=document, layer=1, style={
        'fill_opacity': 0.5, 'effects': [{'type': 'drop_shadow', 'size': 6}]})
    app.call('set_layer_properties', document_id=document, index=1, blend_mode='multiply',
             opacity=0.8)
    return document


def item_save(app, document):
    plain = app.call('save_style_preset', document_id=document, layer=1, name='Shadow Only')
    check('SAVE without blending', plain['preset']['blending'] is None
          and plain['preset']['effects'] == ['drop_shadow'] and plain['file'], json.dumps(plain))
    row = row_named(app, 'Shadow Only')
    check('SAVE without blending resets fill', row and row['style']['fill_opacity'] == 1,
          json.dumps(row and row['style']))
    blended = app.call('save_style_preset', document_id=document, layer=1, name='Shadow Blend',
                       include_blending=True)
    row = row_named(app, 'Shadow Blend')
    check('SAVE with blending', blended['preset']['blending'] == {'mode': 'multiply',
                                                                'opacity': 0.8}
          and row and row['style']['fill_opacity'] == 0.5, json.dumps(blended))


def item_replace(app, document):
    before = layers(app, document)
    count = history(app, document)
    reply = app.call('apply_style_preset', document_id=document, layers=[1, 2], name='Soft Drop')
    after = layers(app, document)
    check('REPLACE one history step', history(app, document) == count + 1
          and reply['layers'] == [1, 2], json.dumps(reply))
    check('REPLACE layer 1 keeps its fill', after[1]['style']['fill_opacity'] == 0.5
          and effects_of(after[1]) == ['drop_shadow'], json.dumps(after[1].get('style')))
    check('REPLACE layer 2 styled', effects_of(after[2]) == ['drop_shadow']
          and after[2]['style']['effects'][0]['size'] == 14, json.dumps(after[2].get('style')))
    app.call('undo', document_id=document)
    undone = layers(app, document)
    check('REPLACE one undo restores both', undone[1].get('style') == before[1].get('style')
          and undone[2].get('style') == before[2].get('style'))


def item_blend(app, document):
    app.call('apply_style_preset', document_id=document, layers=[1], name='Shadow Blend')
    app.call('set_layer_properties', document_id=document, index=1, blend_mode='normal',
             opacity=1)
    before = layers(app, document)
    count = history(app, document)
    app.call('apply_style_preset', document_id=document, layers=[1, 2], name='Shadow Blend')
    after = layers(app, document)
    check('BLEND one history step', history(app, document) == count + 1)
    check('BLEND both modes and opacities', all(
        after[i]['blend_mode'].lower() == 'multiply' and after[i]['opacity'] == 0.8
        for i in (1, 2)), json.dumps([(after[i]['blend_mode'], after[i]['opacity'])
                                      for i in (1, 2)]))
    check('BLEND layer 1 style unchanged, layer 2 styled',
          after[1].get('style') == before[1].get('style')
          and after[2]['style']['fill_opacity'] == 0.5 and effects_of(after[2]) == ['drop_shadow'])


def item_skip(app, document):
    reply = app.call('apply_style_preset', document_id=document, layers=[2, 3], name='Soft Drop')
    skipped = reply.get('skipped') or []
    check('SKIP applies to the pixel and the adjustment layer', reply['layers'] == [2, 3],
          json.dumps(reply))
    check('SKIP skips nothing on RGB', skipped == [], json.dumps(skipped))
    check('SKIP the adjustment layer holds the effects',
          effects_of(layers(app, document)[3]) == ['drop_shadow'])


def item_add(app, document):
    app.call('apply_style_preset', document_id=document, layers=[2], name='Soft Drop',
             add=True)
    after = layers(app, document)
    check('ADD stacks a second drop shadow', effects_of(after[2]) == ['drop_shadow', 'drop_shadow'],
          json.dumps(effects_of(after[2])))
    app.call('apply_style_preset', document_id=document, layers=[2], name='Hairline Frame')
    count = history(app, document)
    again = app.call('apply_style_preset', document_id=document, layers=[2],
                     name='Hairline Frame')
    check('ADD identical replace is unchanged', again.get('unchanged') is True
          and history(app, document) == count, json.dumps(again))


def item_library(app, presets_dir):
    app.call('rename_style_preset', name='Shadow Only', new_name='Shadow Renamed')
    check('LIBRARY rename', row_named(app, 'Shadow Renamed') is not None
          and row_named(app, 'Shadow Only') is None)
    app.call('rename_style_preset', name='Shadow Renamed', new_group='Mine')
    check('LIBRARY move to group', row_named(app, 'Shadow Renamed', 'Mine') is not None
          and 'Mine' in presets(app)['groups'])
    app.call('rename_style_preset', name='Shadow Renamed', group='Mine', new_group='')
    check('LIBRARY ungroup', row_named(app, 'Shadow Renamed') is not None)
    app.call('delete_style_preset', name='Shadow Renamed')
    check('LIBRARY delete', row_named(app, 'Shadow Renamed') is None)
    (presets_dir / 'zz-broken.json').write_text('not json')
    broken = presets(app)['broken']
    check('LIBRARY broken row', len(broken) == 1 and broken[0]['code'] == 'not_json'
          and broken[0]['file'] == 'zz-broken.json', json.dumps(broken))
    app.call('delete_style_preset', file='zz-broken.json')
    check('LIBRARY broken file cleared', presets(app)['broken'] == [])


def item_refuse(app, document, folder):
    text = refusal(app, 'save_style_preset', document_id=document, layer=1, name='X',
                   group='default styles')
    check('REFUSE reserved save group', 'built-in style group' in text, text)
    text = refusal(app, 'save_style_preset', document_id=document, layer=1, name='a/b')
    check('REFUSE slash in name', 'cannot contain “/”' in text, text)
    text = refusal(app, 'rename_style_preset', name='Shadow Blend', new_group='Default Styles')
    check('REFUSE reserved rename group', 'built-in style group' in text, text)
    text = refusal(app, 'rename_style_preset', name='Soft Drop', new_name='Mine')
    check('REFUSE built-in rename', 'cannot be changed' in text, text)
    text = refusal(app, 'delete_style_preset', name='Soft Drop', group=BUILTIN)
    check('REFUSE built-in delete', 'cannot be changed' in text, text)
    path = folder / 'reserved.asl'
    path.write_bytes(drive_library())
    text = refusal(app, 'import_style_presets', path=str(path), group='DEFAULT STYLES')
    check('REFUSE reserved import group', 'built-in style group' in text, text)


def item_import(app, folder):
    path = folder / 'Drive Styles.asl'
    path.write_bytes(drive_library())
    reply = app.call('import_style_presets', path=str(path))
    names = [(r['name'], r['group']) for r in reply['imported']]
    check('IMPORT into the file name group', names == [
        ('Imported Shadow', 'Drive Styles'), ('Imported Emboss', 'Drive Styles'),
        ('Imported Blend', 'Drive Styles')], json.dumps(reply))
    check('IMPORT drops the Stroke Emboss bevel', reply['dropped'] == [
        'Imported Emboss: Bevel & Emboss: bevel uses Stroke Emboss, which Doctored does not '
        'render', 'Off-Name: Drop Shadow: layer effects are switched off'],
        json.dumps(reply['dropped']))
    check('IMPORT skips the empty style', reply['skipped'] == [
        'Off-Name: nothing Doctored models is left'], json.dumps(reply['skipped']))
    emboss = row_named(app, 'Imported Emboss', 'Drive Styles')
    check('IMPORT keeps the stroke', emboss and emboss['effects'] == ['stroke'])
    blend = row_named(app, 'Imported Blend', 'Drive Styles')
    check('IMPORT blending', blend and blend['blending'] == {'mode': 'multiply', 'opacity': 0.7},
          json.dumps(blend and blend['blending']))
    again = app.call('import_style_presets', path=str(path))
    check('IMPORT suffixes a taken name', [r['name'] for r in again['imported']] == [
        'Imported Shadow 2', 'Imported Emboss 2', 'Imported Blend 2'], json.dumps(again))
    check('IMPORT names a drop by the suffixed name', again['dropped'][:1] == [
        'Imported Emboss 2: Bevel & Emboss: bevel uses Stroke Emboss, which Doctored does not '
        'render'], json.dumps(again['dropped']))
    reserved = folder / 'default styles.asl'
    reserved.write_bytes(drive_library())
    third = app.call('import_style_presets', path=str(reserved))
    check('IMPORT defaulted reserved group', {r['group'] for r in third['imported']}
          == {'Default Styles 2'}, json.dumps(third))


def item_export(app, folder):
    listed = presets(app, include_style=True)['presets']
    for group, target in ((BUILTIN, 'Reimported Defaults'), ('Drive Styles', 'Reimported Drive')):
        exported = [r for r in listed if r['group'] == group]
        path = folder / f'{target}.asl'
        reply = app.call('export_style_presets', path=str(path), group=group)
        check(f'EXPORT {group} writes every preset', reply['exported'] == len(exported)
              and reply['skipped'] == [] and path.is_file() and len(exported) >= 3,
              json.dumps(reply))
        back = app.call('import_style_presets', path=str(path), group=target)
        check(f'EXPORT {group} reimports every name', [r['name'] for r in back['imported']]
              == [r['name'] for r in exported] and back['dropped'] == []
              and back['skipped'] == [], json.dumps(back))
        restored = {r['name']: r for r in presets(app, include_style=True, group=target)['presets']}
        mismatched = [r['name'] for r in exported
                      if r['name'] not in restored
                      or restored[r['name']]['style'] != r['style']
                      or restored[r['name']]['blending'] != r['blending']]
        check(f'EXPORT {group} restores every style and blending', mismatched == [],
              json.dumps(mismatched))


def item_depths(app):
    for label, extra, name, wanted in (
            ('RGB16', dict(depth=16), 'Sticker Cut', ['drop_shadow', 'stroke', 'stroke']),
            ('RGB32', dict(depth=32), 'Soft Drop', ['drop_shadow']),
            ('CMYK8', dict(color_mode='cmyk'), 'Soft Drop', ['drop_shadow'])):
        document = new_document(app, **extra)
        app.call('new_layer', document_id=document)
        top = len(layers(app, document)) - 1
        reply = app.call('apply_style_preset', document_id=document, layers=[top], name=name)
        check(f'DEPTHS {label}', effects_of(layers(app, document)[top]) == wanted,
              json.dumps(reply))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', default=str(app_under_test.default_app_bundle()))
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser() if args.receipts else Path(
        tempfile.mkdtemp(prefix='rz-style-presets-'))
    folder = output / 'work'
    folder.mkdir(parents=True)
    presets_dir = folder / 'app' / HostApp.PRESET_DIRECTORIES['DR_STYLE_PRESETS_DIR']
    isolated_before = library_state()
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        item_list(app)
        document = styled_document(app)
        item_save(app, document)
        item_replace(app, document)
        item_blend(app, document)
        item_skip(app, document)
        item_add(app, document)
        item_library(app, presets_dir)
        item_refuse(app, document, folder)
        item_import(app, folder)
        item_export(app, folder)
        item_depths(app)
        saved = sorted((r['name'], r['group']) for r in presets(app)['presets']
                       if not r['builtin'])
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        again = sorted((r['name'], r['group']) for r in presets(app)['presets']
                       if not r['builtin'])
        check('RELAUNCH lists the saved presets', again == saved and len(saved) >= 10,
              f'{len(saved)} presets')
    check('ISOLATED the user library is untouched', library_state() == isolated_before,
          str(USER_LIBRARY))
    check('ISOLATED the private library was used', presets_dir.is_dir())
    if FAILURES:
        print(f'FAILED: {", ".join(FAILURES)}', flush=True)
        raise SystemExit(1)
    print('ALL ITEMS PASS', flush=True)


if __name__ == '__main__':
    main()
