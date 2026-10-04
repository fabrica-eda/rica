"""Binary Texo checkpoints (CBOR + Zstd), with legacy JSON read support.
Run in nix develop for cbor2 and zstandard. No expanded JSON staging file.
"""
import io
import json
import os
from pathlib import Path
import tempfile

MAGIC = b'TEXOCP\x01\n'

def load_checkpoint(path):
    with Path(path).open('rb') as stream:
        if stream.read(len(MAGIC)) != MAGIC:
            stream.seek(0)
            return json.load(stream)
        import cbor2
        import zstandard
        class CheckedFrame(io.RawIOBase):
            def __init__(self):
                self.decoder = zstandard.ZstdDecompressor().decompressobj()
                self.pending = b''
                self.finished = False
            def readable(self):
                return True
            def readinto(self, buffer):
                while not self.pending and not self.finished:
                    if self.decoder.eof:
                        if self.decoder.unused_data or stream.read(1):
                            raise ValueError('trailing compressed checkpoint data')
                        self.finished = True
                        break
                    chunk = stream.read(16384)
                    if not chunk:
                        raise ValueError('truncated checkpoint frame')
                    self.pending = self.decoder.decompress(chunk)
                length = min(len(buffer), len(self.pending))
                buffer[:length] = self.pending[:length]
                self.pending = self.pending[length:]
                return length
        with io.BufferedReader(CheckedFrame()) as decoder:
            value = cbor2.load(decoder)
            if decoder.read(1):
                raise ValueError('trailing checkpoint data')
            return value

def save_checkpoint(path, value):
    import cbor2
    import zstandard
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix='.texo-checkpoint-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(MAGIC)
            with zstandard.ZstdCompressor(level=3, write_checksum=True).stream_writer(stream, closefd=False) as encoder:
                cbor2.dump(value, encoder)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
