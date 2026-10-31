"""Secondary-engine check of sRGB2014's unnormalized relative PCS matrix.

Requires local Little CMS 2, validation only. The strict f64 sampled-curve
contract is checked by generate_real_icc_references.py and the color tests;
this check only uses exact curve endpoints to isolate matrix/white semantics.
"""
import ctypes as C
import ctypes.util
from pathlib import Path
import struct


def main():
    library = ctypes.util.find_library('lcms2')
    if not library:
        raise SystemExit('Little CMS 2 is required for this optional cross-check')
    cms = C.CDLL(library)
    signatures = {
        'cmsOpenProfileFromMem': (C.c_void_p, [C.c_void_p, C.c_uint32]),
        'cmsCreateXYZProfile': (C.c_void_p, []),
        'cmsCreateTransform': (C.c_void_p, [C.c_void_p, C.c_uint32, C.c_void_p, C.c_uint32, C.c_uint32, C.c_uint32]),
        'cmsDoTransform': (None, [C.c_void_p, C.c_void_p, C.c_void_p, C.c_uint32]),
        'cmsDeleteTransform': (None, [C.c_void_p]),
        'cmsCloseProfile': (C.c_int, [C.c_void_p]),
        'cmsGetEncodedCMMversion': (C.c_uint32, []),
    }
    for name, (result, args) in signatures.items():
        fn = getattr(cms, name)
        fn.restype, fn.argtypes = result, args
    root = Path(__file__).resolve().parents[2]
    payload = (root/'extensions/crates/xgpui-color/tests/reference/real-icc/sRGB2014.icc').read_bytes()
    tags = {payload[132+12*i:136+12*i]: payload[offset:offset+size]
            for i in range(struct.unpack_from('>I', payload, 128)[0])
            for offset, size in [struct.unpack_from('>II', payload, 136+12*i)]}
    columns = [tuple(v/65536 for v in struct.unpack_from('>3i', tags[tag], 8))
               for tag in [b'rXYZ', b'gXYZ', b'bXYZ']]
    source, destination = cms.cmsOpenProfileFromMem(payload, len(payload)), cms.cmsCreateXYZProfile()
    transform = None
    try:
        if not source or not destination:
            raise RuntimeError('Could not open validation profiles')
        rgb_double, xyz_double = (1<<22)|(4<<16)|(3<<3), (1<<22)|(9<<16)|(3<<3)
        # Relative colorimetric, NOOPTIMIZE | NOCACHE, no BPC.
        transform = cms.cmsCreateTransform(source, rgb_double, destination, xyz_double, 1, 0x140)
        if not transform:
            raise RuntimeError('Could not create validation transform')
        print(f'Little CMS encoded version: {cms.cmsGetEncodedCMMversion()}')
        for rgb in [(0.,0.,0.), (1.,0.,0.), (0.,1.,0.), (0.,0.,1.), (1.,1.,1.)]:
            expected = [sum(columns[j][i]*rgb[j] for j in range(3)) for i in range(3)]
            actual = (C.c_double*3)()
            cms.cmsDoTransform(transform, (C.c_double*3)(*rgb), actual, 1)
            budget = 16.*2**-23*sum(rgb)
            for a, e in zip(actual, expected):
                assert abs(a-e) <= budget, (rgb, list(actual), expected, budget)
            print(f'{rgb}: PCS {list(actual)}; exact-tag reference {expected}')
        print('PASS: relative PCS retains the encoded matrix white offset.')
    finally:
        if transform:
            cms.cmsDeleteTransform(transform)
        for profile in [source, destination]:
            if profile:
                cms.cmsCloseProfile(profile)


if __name__ == '__main__':
    main()
