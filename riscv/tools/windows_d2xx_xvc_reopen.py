#!/usr/bin/env python3
"""XVC bridge that opens the FTDI handle only for an active JTAG client.

Keep the characterized shift implementation unchanged. A failed USB transfer
aborts its connection: shifts are never retried with ambiguous TAP state.
"""
import socket
import sys
import windows_d2xx_xvc as original


class ClientJtag:
    def __init__(self, arguments):
        self.arguments = arguments
        self.device = None

    def get_device(self):
        if self.device is None:
            self.device = original.D2xxMpsse(
                self.arguments.device_index, self.arguments.initial_hz
            )
        return self.device

    def set_clock(self, hz):
        return self.get_device().set_clock(hz)

    def shift(self, bit_count, tms, tdi):
        return self.get_device().shift(bit_count, tms, tdi)

    def close(self):
        if self.device is not None:
            self.device.close()


def serve(arguments):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((arguments.listen, arguments.port))
        listener.listen(1)
        print(f"D2XX XVC listening on {arguments.listen}:{arguments.port}; per-client FTDI handles", flush=True)
        while True:
            connection, peer = listener.accept()
            connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            jtag = ClientJtag(arguments)
            try:
                with connection:
                    original.client_loop(connection, peer, jtag, arguments.max_bits)
            except EOFError:
                print("XVC client disconnected", flush=True)
            except (original.D2xxError, OSError, ValueError) as error:
                print(f"XVC connection aborted: {error}", file=sys.stderr, flush=True)
            finally:
                jtag.close()


if __name__ == '__main__':
    original.serve = serve
    original.main()
