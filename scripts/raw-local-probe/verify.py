"""Exact native-payload validation, followed by the independent camera oracle."""
from array import array
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from native import local_asset, read_mask


def values(path):
    result = array('H')
    result.frombytes(Path(path).read_bytes())
    if sys.byteorder != 'little':
        result.byteswap()
    return result


def verify_host_exports(result, reference, output):
    rows = []
    for case in result['cameras']:
        for key in ['mixed', 'vignette']:
            parsed, asset, masks = local_asset(case[key + '_native'])
            expected = values(case[key + '_values'])
            assert asset['samples'] == expected
            assert len(expected) == case['width'] * case['height'] * 4
            assert asset['raw']['descriptor']['content_sha256'] == '6c46f9854033e7623ebbb5de8c951df6762ca44f05d45035ccc5cb7e9f4e5692'
            for stored, record in zip(masks, case['masks'], strict=True):
                original = read_mask(Path(record['path']).read_bytes())
                assert stored['samples'] == original['samples'] and stored['sha256'] == original['sha256']
            rows.append({'case': case['space'], 'depth': case['depth'], 'stage': key,
                'exact_source_components': len(expected), 'exact_masks': len(masks),
                'profile_sha256': asset['profile_sha256']})
    source = Path(__file__).with_name('oracle.swift')
    captured = output / 'oracle.swift'
    shutil.copy2(source, captured)
    digest = hashlib.sha256(captured.read_bytes()).hexdigest()
    assert hashlib.sha256(source.read_bytes()).hexdigest() == digest
    binary = output / 'independent-oracle'
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='15.0')
    with (output / 'oracle-build.log').open('w') as log:
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
            '-module-cache-path', str(output / 'oracle-module-cache'), str(captured), '-o', str(binary)],
            check=True, env=environment, stdout=log, stderr=subprocess.STDOUT)
    report = output / 'independent-oracle.json'
    with (output / 'oracle-run.log').open('w') as log:
        subprocess.run([str(binary), str(output / 'results/host-report.json'), str(reference), str(report)],
            check=True, env=environment, stdout=log, stderr=subprocess.STDOUT, timeout=1200)
    oracle = json.loads(report.read_text())
    assert oracle['passed']
    assert len(oracle['cases']) == 4
    for case in oracle['cases']:
        optical = case['pinned_nikon_explicit_optics']
        assert optical['max_native_error'] <= 1
        assert all(value > 1024 for value in optical['displaced_per_channel'])
    assert hashlib.sha256(source.read_bytes()).hexdigest() == digest, 'Used oracle source changed'
    geometry_rows = []
    for group in result.get('geometry', []):
        assert group['passed']
        for row in group['groups']:
            parsed, asset, masks = local_asset(row['native'], recipe_versions=(4,))
            assert asset['raw']['descriptor']['content_sha256'] == \
                'de54f3e1e3ee648da1b28f275fc3d652e79407edb54105e07d2dc4f941191624'
            assert hashlib.sha256(asset['samples'].tobytes()).hexdigest() == row['reviewed_sha256']
            expected_mask = read_mask(Path(row['after_mask']).read_bytes())
            assert len(masks) == 1 and masks[0]['samples'] == expected_mask['samples']
            geometry_rows.append({'name': row['name'], 'depth': row['depth'],
                                  'exact_source_components': len(asset['samples']),
                                  'exact_mask_samples': len(expected_mask['samples'])})
    geometry_source = Path(__file__).with_name('geometry-reference.swift')
    geometry_copy = output / 'geometry-reference.swift'
    shutil.copy2(geometry_source, geometry_copy)
    geometry_hash = hashlib.sha256(geometry_copy.read_bytes()).hexdigest()
    geometry_binary = output / 'independent-geometry-reference'
    with (output / 'geometry-reference-build.log').open('w') as log:
        subprocess.run(['xcrun', 'swiftc', '-O', '-parse-as-library', '-target', 'arm64-apple-macos15.0',
            '-module-cache-path', str(output / 'geometry-reference-module-cache'),
            str(geometry_copy), '-o', str(geometry_binary)], check=True, env=environment,
            stdout=log, stderr=subprocess.STDOUT)
    geometry_report = output / 'independent-geometry-reference.json'
    with (output / 'geometry-reference-run.log').open('w') as log:
        subprocess.run([str(geometry_binary), str(output / 'results/host-report.json'),
            str(geometry_report)], check=True, env=environment, stdout=log,
            stderr=subprocess.STDOUT, timeout=1200)
    geometry_numeric = json.loads(geometry_report.read_text())
    assert geometry_numeric['passed'] and geometry_numeric['max_mask_error'] <= 1
    assert len(geometry_numeric['actual_input_icc_reports']) == 8
    assert geometry_numeric['max_native_error'] <= 3
    assert all(row['old_policy_difference'] >= 128
               for row in geometry_numeric['actual_input_icc_reports'])
    assert hashlib.sha256(geometry_source.read_bytes()).hexdigest() == geometry_hash
    return {'native': rows, 'camera_pipeline': oracle, 'oracle_source_sha256': digest,
        'oracle_binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'report': str(report),
        'geometry_native': geometry_rows, 'geometry_numeric': geometry_numeric,
        'geometry_reference_source_sha256': geometry_hash,
        'geometry_reference_binary_sha256': hashlib.sha256(geometry_binary.read_bytes()).hexdigest(),
        'geometry_reference_report': str(geometry_report)}
