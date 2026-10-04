"""Format integrity tests; run with nix develop --command python3 -m unittest discover -s riscv/tools -p test_checkpoint_io.py."""
import json
from pathlib import Path
import tempfile
import unittest
from checkpoint_io import load_checkpoint, save_checkpoint, MAGIC

class CheckpointIoTests(unittest.TestCase):
    def test_binary_and_legacy(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'design.txcp'
            value = {'wire': 2**64-1, 'slack': -2796, 'route': [0, 1, 2]}
            save_checkpoint(path, value)
            self.assertTrue(path.read_bytes().startswith(MAGIC))
            self.assertEqual(load_checkpoint(path), value)
            old = Path(directory)/'old.json'
            old.write_text(json.dumps(value))
            self.assertEqual(load_checkpoint(old), value)

    def test_incomplete_footer_and_corruption(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'design.txcp'
            save_checkpoint(path, {'data': [123]*1000})
            raw = path.read_bytes()
            for length in [0,7,8,len(raw)-1,len(raw)-4]:
                with self.subTest(length=length):
                    path.write_bytes(raw[:length])
                    with self.assertRaises(Exception):load_checkpoint(path)
            path.write_bytes(raw[:-1] + bytes([raw[-1]^1]))
            with self.assertRaises(Exception):load_checkpoint(path)

    def test_failed_write_preserves_previous_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'design.txcp'
            save_checkpoint(path, {'old': True})
            raw = path.read_bytes()
            with self.assertRaises(Exception):save_checkpoint(path, {'partial': 1, 'invalid': object()})
            self.assertEqual(path.read_bytes(), raw)
            self.assertEqual(len(list(Path(directory).iterdir())), 1)

if __name__ == '__main__':unittest.main()
