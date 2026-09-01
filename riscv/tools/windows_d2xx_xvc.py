#!/usr/bin/env python3
"""Minimal XVC server for an FT2232H using the Windows D2XX driver.

The ECP5 EVN exposes its JTAG chain on interface A of the on-board FT2232H.
Official Windows openFPGALoader builds do not contain the XVC server, while
replacing the board's D2XX driver with WinUSB is unnecessarily disruptive.
This bridge keeps the installed D2XX driver and translates XVC shifts to MPSSE.
"""

from __future__ import annotations

import argparse
import ctypes
import math
import socket
import struct
import sys
import time
from typing import NoReturn


FT_OK = 0
FT_PURGE_RX = 1
FT_PURGE_TX = 2
FT_BITMODE_RESET = 0
FT_BITMODE_MPSSE = 2
FT_FLOW_RTS_CTS = 0x0100

MPSSE_SET_BITS_LOW = 0x80
MPSSE_SET_BITS_HIGH = 0x82
MPSSE_LOOPBACK_END = 0x85
MPSSE_TCK_DIVISOR = 0x86
MPSSE_SEND_IMMEDIATE = 0x87
MPSSE_DISABLE_DIVIDE_BY_5 = 0x8A
MPSSE_DISABLE_3_PHASE = 0x8D
MPSSE_DISABLE_ADAPTIVE = 0x97

# Clock TMS LSB-first, write on the falling edge, and read on the rising edge.
MPSSE_CLOCK_TMS_READ = 0x6B

DEFAULT_PORT = 3721
DEFAULT_MAX_BITS = 65_536
MPSSE_GROUP_BITS = 6
MPSSE_GROUPS_PER_TRANSFER = 8_192


class D2xxError(RuntimeError):
    pass


class D2xxMpsse:
    def __init__(self, device_index: int, initial_hz: int) -> None:
        if sys.platform != "win32":
            raise D2xxError("the D2XX XVC bridge must run on Windows")

        self.dll = ctypes.WinDLL("ftd2xx.dll")
        self._declare_api()
        self._validate_device(device_index)
        self.handle = ctypes.c_void_p()
        self._check(self.dll.FT_Open(device_index, ctypes.byref(self.handle)), "FT_Open")
        try:
            self._initialize(initial_hz)
        except BaseException:
            self.close()
            raise

    def _declare_api(self) -> None:
        void_p = ctypes.c_void_p
        uint32 = ctypes.c_uint32
        uint8 = ctypes.c_ubyte
        uint32_p = ctypes.POINTER(uint32)

        self.dll.FT_CreateDeviceInfoList.argtypes = [uint32_p]
        self.dll.FT_CreateDeviceInfoList.restype = uint32
        self.dll.FT_GetDeviceInfoDetail.argtypes = [
            uint32,
            uint32_p,
            uint32_p,
            uint32_p,
            uint32_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.POINTER(void_p),
        ]
        self.dll.FT_GetDeviceInfoDetail.restype = uint32
        self.dll.FT_Open.argtypes = [ctypes.c_int, ctypes.POINTER(void_p)]
        self.dll.FT_Open.restype = uint32
        self.dll.FT_Close.argtypes = [void_p]
        self.dll.FT_Close.restype = uint32
        self.dll.FT_ResetDevice.argtypes = [void_p]
        self.dll.FT_ResetDevice.restype = uint32
        self.dll.FT_SetUSBParameters.argtypes = [void_p, uint32, uint32]
        self.dll.FT_SetUSBParameters.restype = uint32
        self.dll.FT_SetChars.argtypes = [void_p, uint8, uint8, uint8, uint8]
        self.dll.FT_SetChars.restype = uint32
        self.dll.FT_SetTimeouts.argtypes = [void_p, uint32, uint32]
        self.dll.FT_SetTimeouts.restype = uint32
        self.dll.FT_SetLatencyTimer.argtypes = [void_p, uint8]
        self.dll.FT_SetLatencyTimer.restype = uint32
        self.dll.FT_SetFlowControl.argtypes = [void_p, ctypes.c_ushort, uint8, uint8]
        self.dll.FT_SetFlowControl.restype = uint32
        self.dll.FT_SetResetPipeRetryCount.argtypes = [void_p, uint32]
        self.dll.FT_SetResetPipeRetryCount.restype = uint32
        self.dll.FT_SetBitMode.argtypes = [void_p, uint8, uint8]
        self.dll.FT_SetBitMode.restype = uint32
        self.dll.FT_Purge.argtypes = [void_p, uint32]
        self.dll.FT_Purge.restype = uint32
        self.dll.FT_Write.argtypes = [void_p, void_p, uint32, uint32_p]
        self.dll.FT_Write.restype = uint32
        self.dll.FT_Read.argtypes = [void_p, void_p, uint32, uint32_p]
        self.dll.FT_Read.restype = uint32

    @staticmethod
    def _check(status: int, operation: str) -> None:
        if status != FT_OK:
            raise D2xxError(f"{operation} failed with D2XX status {status}")

    def _validate_device(self, device_index: int) -> None:
        count = ctypes.c_uint32()
        self._check(
            self.dll.FT_CreateDeviceInfoList(ctypes.byref(count)),
            "FT_CreateDeviceInfoList",
        )
        if not 0 <= device_index < count.value:
            raise D2xxError(
                f"FTDI device index {device_index} is outside the enumerated range 0..{count.value - 1}"
            )

        flags = ctypes.c_uint32()
        device_type = ctypes.c_uint32()
        device_id = ctypes.c_uint32()
        location = ctypes.c_uint32()
        serial = ctypes.create_string_buffer(64)
        description = ctypes.create_string_buffer(64)
        existing_handle = ctypes.c_void_p()
        self._check(
            self.dll.FT_GetDeviceInfoDetail(
                device_index,
                ctypes.byref(flags),
                ctypes.byref(device_type),
                ctypes.byref(device_id),
                ctypes.byref(location),
                serial,
                description,
                ctypes.byref(existing_handle),
            ),
            "FT_GetDeviceInfoDetail",
        )
        decoded_description = description.value.decode(errors="replace")
        if (
            device_type.value != 6
            or device_id.value != 0x0403_6010
            or not decoded_description.endswith(" A")
        ):
            raise D2xxError(
                "refusing to drive an unexpected FTDI device: "
                f"index={device_index}, type={device_type.value}, "
                f"id=0x{device_id.value:08x}, description={decoded_description!r}"
            )
        self.device_description = decoded_description
        self.device_serial = serial.value.decode(errors="replace")

    def _initialize(self, initial_hz: int) -> None:
        self._check(self.dll.FT_ResetDevice(self.handle), "FT_ResetDevice")
        self._check(
            # D2XX only supports the input-transfer size here; the output
            # argument is reserved and must be zero.
            self.dll.FT_SetUSBParameters(self.handle, 65_536, 0),
            "FT_SetUSBParameters",
        )
        self._check(self.dll.FT_SetChars(self.handle, 0, 0, 0, 0), "FT_SetChars")
        self._check(self.dll.FT_SetTimeouts(self.handle, 5_000, 5_000), "FT_SetTimeouts")
        self._check(self.dll.FT_SetLatencyTimer(self.handle, 2), "FT_SetLatencyTimer")
        self._check(
            self.dll.FT_SetFlowControl(self.handle, FT_FLOW_RTS_CTS, 0, 0),
            "FT_SetFlowControl",
        )
        self._check(
            self.dll.FT_SetResetPipeRetryCount(self.handle, 100),
            "FT_SetResetPipeRetryCount",
        )
        self._check(self.dll.FT_SetBitMode(self.handle, 0, FT_BITMODE_RESET), "FT_SetBitMode(reset)")
        self._check(self.dll.FT_Purge(self.handle, FT_PURGE_RX | FT_PURGE_TX), "FT_Purge")
        time.sleep(0.05)
        self._check(self.dll.FT_SetBitMode(self.handle, 0, FT_BITMODE_MPSSE), "FT_SetBitMode(MPSSE)")
        time.sleep(0.05)
        self._check(self.dll.FT_Purge(self.handle, FT_PURGE_RX | FT_PURGE_TX), "FT_Purge(MPSSE)")

        # A deliberately invalid MPSSE command must be echoed as FA AA.  This
        # catches a wrong FTDI interface before any JTAG pins are driven.
        self.write(b"\xAA\x87")
        sync = self.read_exact(2)
        if sync != b"\xFA\xAA":
            raise D2xxError(f"MPSSE synchronization failed: received {sync.hex(' ')}")

        self.set_clock(initial_hz)
        # Generic FT2232/openFPGALoader cable mapping:
        # ADBUS0=TCK out, ADBUS1=TDI out, ADBUS2=TDO in, ADBUS3=TMS out.
        # Keep every ACBUS pin as an input; none is part of the EVN JTAG link.
        self.write(
            bytes(
                [
                    MPSSE_SET_BITS_LOW,
                    0x08,
                    0x0B,
                    MPSSE_SET_BITS_HIGH,
                    0x00,
                    0x00,
                    MPSSE_LOOPBACK_END,
                ]
            )
        )

    def close(self) -> None:
        if not getattr(self, "handle", None) or not self.handle.value:
            return
        try:
            self.dll.FT_SetBitMode(self.handle, 0, FT_BITMODE_RESET)
            self.dll.FT_Purge(self.handle, FT_PURGE_RX | FT_PURGE_TX)
        finally:
            self.dll.FT_Close(self.handle)
            self.handle = ctypes.c_void_p()

    def write(self, data: bytes) -> None:
        offset = 0
        while offset < len(data):
            # Keeping individual writes below one USB page is reliable with
            # both current and older parent D2XX drivers.
            chunk = data[offset : offset + 4_096]
            buffer = (ctypes.c_ubyte * len(chunk)).from_buffer_copy(chunk)
            written = ctypes.c_uint32()
            self._check(
                self.dll.FT_Write(self.handle, buffer, len(chunk), ctypes.byref(written)),
                "FT_Write",
            )
            if written.value == 0:
                raise D2xxError("FT_Write made no progress")
            offset += written.value

    def read_exact(self, count: int) -> bytes:
        result = bytearray()
        while len(result) < count:
            remaining = count - len(result)
            buffer = (ctypes.c_ubyte * remaining)()
            read = ctypes.c_uint32()
            self._check(
                self.dll.FT_Read(self.handle, buffer, remaining, ctypes.byref(read)),
                "FT_Read",
            )
            if read.value == 0:
                raise D2xxError(f"FT_Read timed out after {len(result)}/{count} bytes")
            result.extend(bytes(buffer[: read.value]))
        return bytes(result)

    def set_clock(self, requested_hz: int) -> int:
        requested_hz = max(1, min(requested_hz, 30_000_000))
        # FT2232H MPSSE: TCK = 60 MHz / (2 * (1 + divisor)). Never exceed
        # the requested clock; the XVC reply reports the actual value.
        divisor = math.ceil(60_000_000 / (2 * requested_hz)) - 1
        divisor = max(0, min(divisor, 0xFFFF))
        actual_hz = 60_000_000 // (2 * (1 + divisor))
        self.write(
            bytes(
                [
                    MPSSE_DISABLE_DIVIDE_BY_5,
                    MPSSE_DISABLE_ADAPTIVE,
                    MPSSE_DISABLE_3_PHASE,
                    MPSSE_TCK_DIVISOR,
                    divisor & 0xFF,
                    divisor >> 8,
                ]
            )
        )
        return actual_hz

    @staticmethod
    def _bit(vector: bytes, index: int) -> int:
        return (vector[index >> 3] >> (index & 7)) & 1

    def shift(self, bit_count: int, tms: bytes, tdi: bytes) -> bytes:
        tdo = bytearray((bit_count + 7) // 8)
        bit_position = 0
        while bit_position < bit_count:
            commands = bytearray()
            groups: list[tuple[int, int]] = []
            while bit_position < bit_count and len(groups) < MPSSE_GROUPS_PER_TRANSFER:
                start = bit_position
                tdi_value = self._bit(tdi, start)
                length = 1
                while (
                    length < MPSSE_GROUP_BITS
                    and start + length < bit_count
                    and self._bit(tdi, start + length) == tdi_value
                ):
                    length += 1

                tms_value = 0
                for local in range(length):
                    tms_value |= self._bit(tms, start + local) << local
                # FTDI AN_129 requires the final TMS value in the following
                # payload bit so the pin remains stable between commands.
                tms_value |= self._bit(tms, start + length - 1) << length
                commands.extend(
                    [MPSSE_CLOCK_TMS_READ, length - 1, tms_value | (tdi_value << 7)]
                )
                groups.append((start, length))
                bit_position += length

            commands.append(MPSSE_SEND_IMMEDIATE)
            self.write(bytes(commands))
            samples = self.read_exact(len(groups))
            for sample, (start, length) in zip(samples, groups, strict=True):
                aligned = sample >> (8 - length)
                for local in range(length):
                    if (aligned >> local) & 1:
                        output = start + local
                        tdo[output >> 3] |= 1 << (output & 7)
        return bytes(tdo)


def recv_exact(connection: socket.socket, count: int) -> bytes:
    result = bytearray()
    while len(result) < count:
        chunk = connection.recv(count - len(result))
        if not chunk:
            raise EOFError
        result.extend(chunk)
    return bytes(result)


def client_loop(
    connection: socket.socket,
    peer: tuple[str, int],
    jtag: D2xxMpsse,
    max_bits: int,
) -> None:
    print(f"XVC client connected: {peer[0]}:{peer[1]}", flush=True)
    while True:
        prefix = recv_exact(connection, 2)
        if prefix == b"ge":
            if recv_exact(connection, 6) != b"tinfo:":
                raise ValueError("malformed getinfo command")
            connection.sendall(f"xvcServer_v1.0:{max_bits}\n".encode())
        elif prefix == b"se":
            command = recv_exact(connection, 9)
            if command[:5] != b"ttck:":
                raise ValueError("malformed settck command")
            requested_period_ns = struct.unpack("<I", command[5:])[0]
            if requested_period_ns == 0:
                raise ValueError("XVC requested a zero TCK period")
            requested_hz = max(1, 1_000_000_000 // requested_period_ns)
            actual_hz = jtag.set_clock(requested_hz)
            actual_period_ns = max(1, math.ceil(1_000_000_000 / actual_hz))
            connection.sendall(struct.pack("<I", actual_period_ns))
            print(
                f"XVC TCK: requested {requested_period_ns} ns, "
                f"actual {actual_period_ns} ns ({actual_hz} Hz)",
                flush=True,
            )
        elif prefix == b"sh":
            if recv_exact(connection, 4) != b"ift:":
                raise ValueError("malformed shift command")
            bit_count = struct.unpack("<I", recv_exact(connection, 4))[0]
            if bit_count > max_bits:
                raise ValueError(f"XVC shift of {bit_count} bits exceeds limit {max_bits}")
            byte_count = (bit_count + 7) // 8
            tms = recv_exact(connection, byte_count)
            tdi = recv_exact(connection, byte_count)
            connection.sendall(jtag.shift(bit_count, tms, tdi))
        else:
            raise ValueError(f"unknown XVC command prefix {prefix!r}")


def serve(arguments: argparse.Namespace) -> NoReturn:
    jtag = D2xxMpsse(arguments.device_index, arguments.initial_hz)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((arguments.listen, arguments.port))
    listener.listen(1)
    print(
        f"D2XX XVC ready on {arguments.listen}:{arguments.port}; "
        f"FTDI device index {arguments.device_index} "
        f"({jtag.device_description}, serial {jtag.device_serial})",
        flush=True,
    )
    try:
        while True:
            connection, peer = listener.accept()
            connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            try:
                with connection:
                    client_loop(connection, peer, jtag, arguments.max_bits)
            except EOFError:
                print(f"XVC client disconnected after a complete request stream", flush=True)
            except D2xxError as error:
                print(f"fatal D2XX error: {error}", file=sys.stderr, flush=True)
                raise
            except Exception as error:
                print(f"XVC client error: {error}", file=sys.stderr, flush=True)
    finally:
        listener.close()
        jtag.close()


def main() -> NoReturn:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--device-index", type=int, default=0)
    parser.add_argument("--initial-hz", type=int, default=6_000_000)
    parser.add_argument("--max-bits", type=int, default=DEFAULT_MAX_BITS)
    serve(parser.parse_args())


if __name__ == "__main__":
    main()
