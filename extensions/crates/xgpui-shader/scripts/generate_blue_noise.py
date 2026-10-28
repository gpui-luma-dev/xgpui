"""Generate the 16x16 toroidal void-and-cluster rank tile (stdlib only).

Algorithm: https://cv.ulichney.com/papers/1993-void-cluster.pdf
Gaussian sigma=1.5; fixed integer hash seeds the half-full initial pattern.
Run with --check to verify the checked-in asset without rewriting it.
"""
import math
from pathlib import Path
import sys

SIDE = 16
COUNT = SIDE * SIDE


def generate():
    kernel = [math.exp(-(min(x, SIDE-x)**2 + min(y, SIDE-y)**2) / 4.5)
              for y in range(SIDE) for x in range(SIDE)]
    weights = [[kernel[((j//SIDE-i//SIDE) % SIDE)*SIDE + (j-i) % SIDE]
                for j in range(COUNT)] for i in range(COUNT)]
    # Integer arithmetic fixes the seed independently of Python's PRNG version.
    order = sorted(range(COUNT), key=lambda i: ((i * 1664525 + 1013904223) & 0xffffffff) ^
                   (((i * 22695477 + 1) & 0xffffffff) >> 7))
    occupied = set(order[:COUNT//2])
    density = [sum(weights[i][j] for i in sorted(occupied)) for j in range(COUNT)]

    def update(i, sign):
        for j in range(COUNT):
            density[j] += sign * weights[i][j]

    def cluster():
        return max(sorted(occupied), key=lambda i: density[i])

    def void():
        return min((i for i in range(COUNT) if i not in occupied), key=lambda i: density[i])

    for _ in range(10000):
        old = cluster()
        occupied.remove(old)
        update(old, -1)
        new = void()
        occupied.add(new)
        update(new, 1)
        if old == new:
            break
    else:
        raise RuntimeError('relaxation did not converge')
    initial = occupied.copy()
    initial_density = density.copy()
    ranks = [None] * COUNT
    for rank in reversed(range(COUNT//2)):
        i = cluster()
        ranks[i] = rank
        occupied.remove(i)
        update(i, -1)
    occupied = initial
    density = initial_density
    for rank in range(COUNT//2, COUNT):
        i = void()
        ranks[i] = rank
        occupied.add(i)
        update(i, 1)
    assert sorted(ranks) == list(range(COUNT))
    return bytes(ranks)


if __name__ == '__main__':
    target = Path(__file__).resolve().parents[1] / 'src/brush/blue_noise.bin'
    data = generate()
    if '--check' in sys.argv:
        assert target.read_bytes() == data, 'blue_noise.bin is stale'
        print('blue_noise.bin reproducible (256 ranks)')
    else:
        target.write_bytes(data)
