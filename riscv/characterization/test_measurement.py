"""Digital protocol/estimator tests only; these are never calibration samples."""
import unittest
from readout import decode
from fit import features,fit
class MeasurementChecks(unittest.TestCase):
 def test_frame_validates_magic_and_fields(self):
  frame=(0x524f4d31<<96)|(25<<64)|(7<<32)|50000
  self.assertEqual(decode(frame<<1),dict(sequence=25,probe=7,count=50000,bit_offset=1))
  self.assertIsNone(decode(0));self.assertIsNone(decode((frame&~0xffffffff)<<1))
  self.assertIsNone(decode(((0x524f4d31<<96)|(25<<64)|(12<<32)|1)<<1))
 def test_loop_features_include_closing_edge(self):
  self.assertEqual(features(dict(sites=['X20/Y6/SLICEA.K0','X24/Y6/SLICEA.K0','X20/Y7/SLICEB.K0'])),[3,8,0,2,0,1])
 def test_fit_does_not_require_negative_costs(self):
  # Synthetic regression test, not an input to any hardware model artifact.
  rows=[dict(features=[n,x,0,0,0,1],loop_delay_ps=400*n+30*x+100) for n in [33,65,97] for x in [32,128,384]]
  c=fit(rows)
  self.assertAlmostEqual(c[0],400);self.assertAlmostEqual(c[1],30);self.assertAlmostEqual(c[5],100)
if __name__=='__main__':unittest.main()
