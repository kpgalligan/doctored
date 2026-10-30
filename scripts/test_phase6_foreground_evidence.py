#!/usr/bin/env python3
"""Independent scalar oracles for the read-only foreground diagnostic."""
import sys
import unittest

sys.dont_write_bytecode = True
from phase6_foreground_evidence import foreground_byte_bounds


class ForegroundEvidenceTests(unittest.TestCase):
    def test_feasible_integer_range_matches_exhaustive_background_interval(self):
        for source in [0, 1, 63, 127, 128, 193, 254, 255]:
            for alpha in [0, 1, 2, 17, 127, 128, 239, 254, 255]:
                feasible = []
                for foreground in range(256):
                    # Independent oracle: varying B across the unit gamut
                    # sweeps this interval of observable source-byte values.
                    minimum = alpha * foreground / 255
                    maximum = minimum + 255 - alpha
                    if maximum >= source - 0.5 and minimum <= source + 0.5:
                        feasible.append(foreground)
                self.assertEqual(foreground_byte_bounds(source, alpha), (min(feasible), max(feasible)))

    def test_opaque_identity_and_hidden_background_limits(self):
        for source in range(256):
            self.assertEqual(foreground_byte_bounds(source, 255), (source, source))
            self.assertEqual(foreground_byte_bounds(source, 0), (0, 255))
            for alpha in [1, 127, 254]:
                lower, upper = foreground_byte_bounds(source, alpha)
                self.assertLessEqual(lower, source)
                self.assertGreaterEqual(upper, source)


if __name__ == "__main__":
    unittest.main()
