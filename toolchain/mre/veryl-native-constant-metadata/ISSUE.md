# Incorrect sign extension of a signed constant in native simulation

On Veryl 0.21.0 (`2e32d53`), the test below expects
`o = ffffffffffffffff` but gets `o = 00000000000000ff`.
The signed 8-bit constant should be sign-extended to 64 bits.
The assertion fails with `interpret`, `cranelift`, and `cc`.

```sh
veryl test --backend interpret --no-capture --seed 1
```

```veryl
module SignedConstant (
    a: input  logic<64>,
    o: output logic<64>,
) {
    const P    : signed logic<8>  = 64'd255;
    let   value: logic       <64> = P;
    assign o = a & value;
}

#[test(test_signed_constant)]
module test_signed_constant {
    inst clk: $tb::clock_gen;

    var a: logic<64>;
    var o: logic<64>;
    inst dut: SignedConstant ( a, o );

    initial {
        a = 64'hffff_ffff_ffff_ffff;
        clk.next();
        $display("a=%h o=%h", a, o);
        $assert(o == 64'hffff_ffff_ffff_ffff);
        $finish();
    }
}
```
