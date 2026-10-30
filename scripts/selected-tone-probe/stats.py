"""Independent scoped-statistics oracles over known integer source samples."""
import math
import struct


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def canvas(shape, offset=(0, 0), clear_hidden=False):
    w, h, _, codes = shape
    result = [(0, 0, 0, 0)] * (w*h)
    for y in range(h):
        for x in range(w):
            cx, cy = x+offset[0], y+offset[1]
            if 0 <= cx < w and 0 <= cy < h:
                p = tuple(codes[(y*w+x)*4:(y*w+x)*4+4])
                result[cy*w+cx] = (0, 0, 0, 0) if clear_hidden and p[3] == 0 else p
    return result


def histogram_reference(pixels, coverage, maximum, stride):
    bins = [[0]*256 for _ in range(4)]
    zeros, maxima, zero_weights, max_weights, sums = ([0]*4 for _ in range(5))
    rgb_weight = alpha_weight = 0
    for i, (p, c) in enumerate(zip(pixels, coverage)):
        weight = c * 257
        if weight == 0:
            continue
        alpha_weight += weight
        if p[3] > 0:
            rgb_weight += weight
        for channel in range(4):
            if channel < 3 and p[3] == 0:
                continue
            sums[channel] += p[channel]*weight
            if p[channel] == 0:
                zeros[channel] += 1
                zero_weights[channel] += weight
            if p[channel] == maximum:
                maxima[channel] += 1
                max_weights[channel] += weight
        if i % stride == 0 and p[3] > 0:
            for channel in range(3):
                index = math.floor(p[channel]*255/maximum+0.5)
                bins[channel][index] += weight
            luma = sum(v*k for v, k in zip(p[:3], map(f32, [0.2126, 0.7152, 0.0722])))
            index = min(255, max(0, math.floor(luma*255/maximum+0.5)))
            bins[3][index] += weight
    return dict(bin_weight_units=bins, full_rgb_weight_units=rgb_weight,
                full_alpha_weight_units=alpha_weight, weighted_code_sums_rgba=sums,
                exact_zero_rgba=zeros, exact_max_rgba=maxima,
                exact_zero_weight_units_rgba=zero_weights, exact_max_weight_units_rgba=max_weights)


def check_histogram(result, pixels, coverage, maximum, stride):
    assert result['schema_version'] == 2 and result['maximum_code'] == maximum, result
    assert result['coverage_denominator'] == 65535
    expected = histogram_reference(pixels, coverage, maximum, stride)
    for field, value in expected.items():
        assert result[field] == value, (field, result[field], value)
    for actual_row, units in zip(result['bins'], expected['bin_weight_units']):
        assert len(actual_row) == 256
        assert all(abs(a-b/65535) < 1e-10 for a, b in zip(actual_row, units))
    return expected


def check_sample(result, pixels, coverage, maximum, x, y, reach, width, height):
    sums = [0]*4
    weight = 0
    for cy in range(max(0,y-reach),min(height,y+reach+1)):
        for cx in range(max(0,x-reach),min(width,x+reach+1)):
            index = cy*width+cx
            c = coverage[index]*257
            weight += c
            for channel in range(4):
                sums[channel] += pixels[index][channel]*c
    assert weight > 0
    assert result['maximum_code'] == maximum
    assert result['weighted_sum_rgba_codes'] == sums, (result,sums)
    assert result['weight_units'] == weight, (result,weight)
    assert result['center_rgba_codes'] == list(pixels[y*width+x])
    assert all(abs(a-b/weight) < 1e-8 for a,b in zip(result['mean_rgba_codes'],sums))
    assert all(abs(a-b/weight/maximum) < 1e-12 for a,b in zip(result['normalized_rgba'],sums))


def run(app, folder, depth, workflow):
    m = workflow
    folder.mkdir()
    path = m.fixture(folder, depth, alpha=True)
    call = m.probe.opened(app,path,depth,folder/'calls.jsonl')
    call('set_layer_properties',index=0,offset_x=-3,offset_y=2)
    raw = m.raw(call,folder,'source')
    pixels = canvas(raw,offset=(-3,2))
    coverage = m.selection(call)
    maximum = (1 << depth)-1
    results = []
    history = m.probe.history(call)
    for selected in [False,True]:
        weights = coverage if selected else [255]*len(pixels)
        for stride in [1,3]:
            result = call('histogram',source='layer',layer=0,selection=selected,stride=stride)
            expected = check_histogram(result,pixels,weights,maximum,stride)
            results.append(dict(kind='raw_layer',selection=selected,stride=stride,
                                rgb_weight=expected['full_rgb_weight_units'],
                                exact_zero_rgba=expected['exact_zero_rgba']))
        for x,y in [(4,4),(8,6)]:
            sample = call('sample_pixel',source='layer',layer=0,selection=selected,x=x,y=y,sample_size=5)
            check_sample(sample,pixels,weights,maximum,x,y,2,m.WIDTH,m.HEIGHT)
            results.append(dict(kind='raw_readout',selection=selected,x=x,y=y))
    # Composite semantics clear fully hidden RGB. Its source must also honor
    # offsets; no displayed byte buffer can replace its native samples.
    composite = canvas(raw,offset=(-3,2),clear_hidden=True)
    result = call('histogram',source='composite',selection=False,stride=3)
    check_histogram(result,composite,[255]*len(pixels),maximum,3)
    assert m.probe.history(call) == history, 'statistics must not edit history'
    results.append(dict(kind='composite',stride=3))

    # The named cut goes INSIDE a group. Its enclosing 50% opacity must
    # remain, while the upper child and a later adjustment must not appear.
    path = m.fixture(folder,depth)
    call = m.probe.opened(app,path,depth,folder/'backdrop-calls.jsonl')
    original = m.raw(call,folder,'backdrop-original')
    call('duplicate_layer',index=0)
    call('apply_filter',layer=1,filter='invert')
    call('group_layers',layers=[0,1],name='Scoped backdrop group')
    call('set_layer_properties',index=2,opacity=0.5)
    call('add_adjustment_layer',layer=2,op='bcs',params=dict(brightness=0.2))
    gated_pixels = [(*p[:3],math.floor(maximum/2+0.5)) for p in canvas(original)]
    coverage = m.selection(call)
    history = m.probe.history(call)
    m.shared.refusal(call,'histogram',source='layer',layer=3)
    m.shared.refusal(call,'sample_pixel',source='layer',layer=3,x=8,y=6)
    for selected in [False,True]:
        weights = coverage if selected else [255]*len(gated_pixels)
        for stride in [1,3]:
            result = call('histogram',source='backdrop',below=1,selection=selected,stride=stride)
            check_histogram(result,gated_pixels,weights,maximum,stride)
            results.append(dict(kind='group_backdrop',selection=selected,stride=stride))
        sample = call('sample_pixel',source='backdrop',below=1,selection=selected,x=8,y=6,sample_size=5)
        check_sample(sample,gated_pixels,weights,maximum,8,6,2,m.WIDTH,m.HEIGHT)
    empty = call('histogram',source='backdrop',below=0,selection=False,stride=3)
    check_histogram(empty,[(0,0,0,0)]*len(gated_pixels),[255]*len(gated_pixels),maximum,3)
    for arguments in [dict(source='backdrop',below=999),dict(source='backdrop'),dict(source='unknown')]:
        m.shared.refusal(call,'histogram',**arguments)
    assert m.probe.history(call) == history
    return dict(passed=True,cases=results,case_count=len(results),coverage_values=sorted(set(coverage)))
