"""Protocol negative controls; these generated test frames are never fit inputs."""
import unittest
from phase_sweep import frame, MAGIC, SAMPLES

class MockXvc:
    def __init__(self, words, offset=1):
        self.value=sum(w<<(32*i+offset) for i,w in enumerate(words))
    def shift(self,tms,tdi):
        assert len(tms)==len(tdi)
        return [0]*3+[(self.value>>i)&1 for i in range(len(tms)-3)]

class ProtocolTest(unittest.TestCase):
    def test_polarity_order_and_max_count(self):
        words=[MAGIC,2,1,SAMPLES,0,SAMPLES//2,1729,391]
        decoded,raw,offset=frame(MockXvc(words),32*len(words))
        self.assertEqual(decoded,words)
        self.assertEqual(offset,1)
        self.assertEqual((int(raw,16)>>1)&0xffffffff,MAGIC)
    def test_bad_magic_rejected(self):
        with self.assertRaises(RuntimeError):
            frame(MockXvc([MAGIC^0x8000,2,1,SAMPLES,0,0,0,0]),256)
    def test_every_allowed_alignment(self):
        for offset in range(5):
            self.assertEqual(frame(MockXvc([MAGIC,0x87654321],offset),64)[0],[MAGIC,0x87654321])

class HoldoutTest(unittest.TestCase):
    def test_failed_holdout_cannot_expand_budget(self):
        # Synthetic fixture only; never stored in the measurements directory.
        import contextlib, io, json, tempfile
        from pathlib import Path
        from phase_budget import build, query
        rows=[dict(id=0,kind='ff_lut_ff',lut_stages=2,holdout=False,usable_boundary=True,
                   observed_aperture_envelope_ps=[500,1000]),
              dict(id=1,kind='ff_lut_ff',lut_stages=2,holdout=True,usable_boundary=True,
                   observed_aperture_envelope_ps=[500,2000])]
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'fixture.json'
            p.write_text(json.dumps(dict(input_sha256={},probes=rows,
                temperature_margin=dict(fraction=.2,additional_guard_ps=200))))
            with contextlib.redirect_stdout(io.StringIO()): result=build(p)
            entry=result['families']['ff_lut_ff/2']
            self.assertEqual(entry['guarded_capture_budget_ps'],1400)
            self.assertFalse(entry['available'])
            with self.assertRaises(ValueError):query(Path(d)/'budget-model.json','ff_lut_ff/2')

if __name__=='__main__':unittest.main()
