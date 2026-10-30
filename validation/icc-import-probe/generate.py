"""Generate synthetic ICC profiles and independent Little CMS float references.

Validation only: requires an installed lcms2 shared library. No product dependency.
Profiles are reopened after serialization so expected values include ICC tag
quantization. Fixed header date/profile ID make fixtures reproducible.
"""
import argparse
import ctypes as C
import ctypes.util
from itertools import product
from pathlib import Path
import struct
import sys
from decimal import Decimal as D, localcontext
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from generate_adobe_rgb_references import inverse

ROOT = Path(__file__).resolve().parent


class XyY(C.Structure):
    _fields_ = [('x', C.c_double), ('y', C.c_double), ('Y', C.c_double)]


class Primaries(C.Structure):
    _fields_ = [('red', XyY), ('green', XyY), ('blue', XyY)]


def multiply(a, b):
    return [[sum(a[i][k]*b[k][j] for k in range(len(b))) for j in range(len(b[0]))] for i in range(len(a))]


def profile_tags(data):
    count = struct.unpack_from('>I', data, 128)[0]
    return {data[132+12*i:136+12*i]: data[offset:offset+size]
            for i in range(count)
            for offset, size in [struct.unpack_from('>II', data, 136+12*i)]}


def colorants(tags):
    columns = [[D(v)/65536 for v in struct.unpack_from('>3i', tags[tag], 8)]
               for tag in [b'rXYZ', b'gXYZ', b'bXYZ']]
    return [[columns[j][i] for j in range(3)] for i in range(3)]


def decode_curve(tag, value):
    x = D.from_float(value)
    if tag[:4] == b'para':
        assert struct.unpack_from('>H', tag, 8)[0] == 3
        g,a,b,c,d = [D(v)/65536 for v in struct.unpack_from('>5i', tag, 12)]
        return (a*x+b)**g if x >= d else c*x
    assert tag[:4] == b'curv'
    count = struct.unpack_from('>I', tag, 8)[0]
    assert count == 257
    values = struct.unpack_from(f'>{count}H', tag, 12)
    position = x*(count-1)
    index = min(int(position), count-2)
    fraction = position-index
    return (D(values[index])*(1-fraction)+D(values[index+1])*fraction)/65535


def generate():
    library = ctypes.util.find_library('lcms2')
    if not library:
        raise SystemExit('Install Little CMS 2 to regenerate these validation fixtures')
    cms = C.CDLL(library)
    signatures = {
        'cmsBuildGamma': (C.c_void_p, [C.c_void_p, C.c_double]),
        'cmsBuildParametricToneCurve': (C.c_void_p, [C.c_void_p, C.c_int, C.POINTER(C.c_double)]),
        'cmsBuildTabulatedToneCurve16': (C.c_void_p, [C.c_void_p, C.c_uint32, C.POINTER(C.c_uint16)]),
        'cmsReadTag': (C.c_void_p, [C.c_void_p, C.c_uint32]),
        'cmsEvalToneCurveFloat': (C.c_float, [C.c_void_p, C.c_float]),
        'cmsFreeToneCurve': (None, [C.c_void_p]),
        'cmsCreateRGBProfile': (C.c_void_p, [C.POINTER(XyY), C.POINTER(Primaries), C.POINTER(C.c_void_p)]),
        'cmsSaveProfileToMem': (C.c_int, [C.c_void_p, C.c_void_p, C.POINTER(C.c_uint32)]),
        'cmsOpenProfileFromMem': (C.c_void_p, [C.c_void_p, C.c_uint32]),
        'cmsCloseProfile': (C.c_int, [C.c_void_p]),
        'cmsCreateTransform': (C.c_void_p, [C.c_void_p, C.c_uint32, C.c_void_p, C.c_uint32, C.c_uint32, C.c_uint32]),
        'cmsDoTransform': (None, [C.c_void_p, C.c_void_p, C.c_void_p, C.c_uint32]),
        'cmsDeleteTransform': (None, [C.c_void_p]),
        'cmsGetEncodedCMMversion': (C.c_uint32, []),
    }
    for name, (result, args) in signatures.items():
        fn = getattr(cms, name)
        fn.restype, fn.argtypes = result, args
    version = cms.cmsGetEncodedCMMversion()
    print(f'Little CMS encoded version: {version}')

    def profile(primaries, gamma, curve_kind="gamma"):
        white = XyY(.3127, .3290, 1.)
        points = Primaries(*(XyY(x, y, 1.) for x, y in primaries))
        curves = []
        for channel in range(1 if curve_kind == "gamma" else 3):
            if curve_kind == 'parametric':
                params = (C.c_double * 5)(2.4, 1/1.055, .055/1.055, 1/12.92, .04045)
                curve = cms.cmsBuildParametricToneCurve(None, 4, params)
            elif curve_kind == 'sampled':
                exponent = [2., 2.2, 1.8][channel]
                samples = (C.c_uint16 * 257)(*(round((i/256)**exponent*65535) for i in range(257)))
                curve = cms.cmsBuildTabulatedToneCurve16(None, 257, samples)
            else:
                curve = cms.cmsBuildGamma(None, gamma)
            if not curve:
                raise RuntimeError('create curve')
            curves.append(curve)
        handle = cms.cmsCreateRGBProfile(C.byref(white), C.byref(points), (C.c_void_p * 3)(*(curves*3 if len(curves) == 1 else curves)))
        for curve in curves:
            cms.cmsFreeToneCurve(curve)
        if not handle:
            raise RuntimeError('create RGB profile')
        try:
            size = C.c_uint32()
            assert cms.cmsSaveProfileToMem(handle, None, C.byref(size))
            buffer = C.create_string_buffer(size.value)
            assert cms.cmsSaveProfileToMem(handle, buffer, C.byref(size))
            data = bytearray(buffer.raw)
            data[24:36] = struct.pack('>6H', 2026, 10, 30, 0, 0, 0)
            data[84:100] = bytes(16)
            return bytes(data)
        finally:
            cms.cmsCloseProfile(handle)

    payloads = {
        'adobe-rgb.icc': profile([(.64,.33),(.21,.71),(.15,.06)], 563/256),
        'linear-p3.icc': profile([(.68,.32),(.265,.69),(.15,.06)], 1.),
        'linear-rec2020.icc': profile([(.708,.292),(.170,.797),(.131,.046)], 1.),
    }
    payloads['parametric-rgb.icc'] = profile([(.64,.33),(.21,.71),(.15,.06)], 1., 'parametric')
    payloads['sampled-rgb.icc'] = profile([(.64,.33),(.21,.71),(.15,.06)], 1., 'sampled')
    buffers = {name: C.create_string_buffer(data) for name, data in payloads.items()}
    handles = {name: cms.cmsOpenProfileFromMem(buffers[name], len(data)) for name, data in payloads.items()}
    if not all(handles.values()):
        raise RuntimeError('reopen serialized profile')
    lines = [f'// Generated by validation/icc-import-probe/generate.py, Little CMS {version}.',
             '// Serialized synthetic ICC profiles; relative colorimetric, RGB double, no optimization.',
             '#[rustfmt::skip]', 'pub const REFERENCES: &[(bool, [f64; 3], [f64; 3])] = &[']
    rgb_double = (1 << 22) | (4 << 16) | (3 << 3)
    curve_lines = [f'// Generated by validation/icc-import-probe/generate.py, Little CMS {version}.',
                   '// (sampled, rec2020, encoded RGB, exact-tag decoded RGB, Little CMS result, exact-tag result)',
                   '#[rustfmt::skip]', '#[allow(clippy::type_complexity)] // Compact generated fixture records.', 'pub const CURVE_REFERENCES: &[(bool, bool, [f64; 3], [f64; 3], [f64; 3], [f64; 3])] = &[']
    try:
        for source_name in ['parametric-rgb.icc', 'sampled-rgb.icc']:
            source_handle = handles[source_name]
            tags = profile_tags(payloads[source_name])
            trcs = [cms.cmsReadTag(source_handle, int.from_bytes(tag, 'big')) for tag in [b'rTRC', b'gTRC', b'bTRC']]
            if not all(trcs):
                raise RuntimeError('read serialized curves')
            inputs = list(product([0., .0001, .001, .18, .5, 1.], repeat=3))
            inputs += [(i/65536,)*3 for i in range(33)]
            boundaries = [.04045, round(.04045*65536)/65536] if source_name.startswith('parametric') else [1/256, 32/256, 128/256, 255/256]
            for boundary in boundaries:
                bits = struct.unpack('<I', struct.pack('<f', boundary))[0]
                for offset in [-1, 0, 1]:
                    v = struct.unpack('<f', struct.pack('<I', bits+offset))[0]
                    inputs.append((v, v, v))
            for rec2020, destination in [(False, 'linear-p3.icc'), (True, 'linear-rec2020.icc')]:
                transform = cms.cmsCreateTransform(source_handle, rgb_double, handles[destination], rgb_double, 1, 0x100 | 0x40)
                if not transform:
                    raise RuntimeError('create curve transform')
                with localcontext() as ctx:
                    ctx.prec = 120
                    conversion = multiply(inverse(colorants(profile_tags(payloads[destination]))), colorants(tags))
                try:
                    for rgb in inputs:
                        src, dst = (C.c_double * 3)(*rgb), (C.c_double * 3)()
                        cms.cmsDoTransform(transform, src, dst, 1)
                        with localcontext() as ctx:
                            ctx.prec = 120
                            decoded_decimal = [decode_curve(tags[tag], rgb[i]) for i, tag in enumerate([b'rTRC',b'gTRC',b'bTRC'])]
                            analytic = [float(row[0]) for row in multiply(conversion, [[v] for v in decoded_decimal])]
                        decoded = [float(v) for v in decoded_decimal]
                        curve_lines.append(f'    ({str(source_name.startswith("sampled")).lower()}, {str(rec2020).lower()}, {list(rgb)!r}, {decoded!r}, {list(dst)!r}, {analytic!r}),')
                finally:
                    cms.cmsDeleteTransform(transform)
        for rec2020, destination in [(False, 'linear-p3.icc'), (True, 'linear-rec2020.icc')]:
            transform = cms.cmsCreateTransform(handles['adobe-rgb.icc'], rgb_double,
                                               handles[destination], rgb_double, 1, 0x100 | 0x40)
            if not transform:
                raise RuntimeError('create transform')
            try:
                for rgb in product([0., .0001, .001, .18, .5, 1.], repeat=3):
                    src, dst = (C.c_double * 3)(*rgb), (C.c_double * 3)()
                    cms.cmsDoTransform(transform, src, dst, 1)
                    lines.append(f'    ({str(rec2020).lower()}, {list(rgb)!r}, {list(dst)!r}),')
            finally:
                cms.cmsDeleteTransform(transform)
    finally:
        for handle in handles.values():
            cms.cmsCloseProfile(handle)
    lines.append('];\n')
    artifacts = {ROOT / 'fixtures' / name: data for name, data in payloads.items()}
    artifacts[ROOT / 'src' / 'reference.rs'] = '\n'.join(lines).encode()
    curve_lines.append('];\n')
    artifacts[ROOT / 'src' / 'curve_reference.rs'] = '\n'.join(curve_lines).encode()
    return artifacts


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    for path, data in generate().items():
        if args.check:
            if path.read_bytes() != data:
                raise SystemExit(f'Fixture differs: {path}')
        else:
            path.write_bytes(data)
    print('ICC fixtures verified.' if args.check else 'ICC fixtures generated.')
