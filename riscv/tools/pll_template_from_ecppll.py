#!/usr/bin/env python3
"""Turn one ecppll result into a U-platform PLL template and provenance record.

Usage: pll_template_from_ecppll.py OUTPUT.json PROVENANCE.json -- ECPPLL-ARGS...

The ecppll output is used unmodified. The template is rejected unless it uses
a 12 MHz reference with CLKI_DIV=1 (phase detector above 10 MHz), internal
CLKOP feedback, a VCO in 400..800 MHz, and zero CPHASE/FPHASE on both fabric
outputs, so CLKOS and CLKOS2 are phase-aligned integer divisions of the VCO.
"""
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ECPPLL = Path('/usr/bin/ecppll')
PARAMETERS = [
    'PLLRST_ENA', 'INTFB_WAKE', 'STDBY_ENABLE', 'DPHASE_SOURCE',
    'OUTDIVIDER_MUXA', 'OUTDIVIDER_MUXB', 'OUTDIVIDER_MUXC', 'OUTDIVIDER_MUXD',
    'CLKI_DIV', 'CLKOP_ENABLE', 'CLKOP_DIV', 'CLKOP_CPHASE', 'CLKOP_FPHASE',
    'CLKOS_ENABLE', 'CLKOS_DIV', 'CLKOS_CPHASE', 'CLKOS_FPHASE',
    'CLKOS2_ENABLE', 'CLKOS2_DIV', 'CLKOS2_CPHASE', 'CLKOS2_FPHASE',
    'FEEDBK_PATH', 'CLKFB_DIV',
]


def require(condition, message):
    if not condition:
        raise SystemExit(f'rejected PLL template: {message}')


def main():
    output, provenance = Path(sys.argv[1]), Path(sys.argv[2])
    require(sys.argv[3] == '--', 'expected -- before ecppll arguments')
    arguments = sys.argv[4:]
    with tempfile.TemporaryDirectory() as temp:
        verilog = Path(temp) / 'pll.v'
        command = [str(ECPPLL), *arguments, '--file', str(verilog)]
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        text = verilog.read_text()
    found = {name: quoted or number for name, quoted, number in
             re.findall(r'\.(\w+)\((?:"([^"]*)"|(-?\d+))\)', text)}
    attributes = dict(re.findall(r'\(\* (\w+)="([^"]*)" \*\)', text))
    parameters = {name: found[name] for name in PARAMETERS}
    integer = lambda name: int(parameters[name])
    reference_mhz = float(attributes['FREQUENCY_PIN_CLKI'])
    vco_mhz = reference_mhz * integer('CLKFB_DIV') * integer('CLKOP_DIV') / integer('CLKI_DIV')
    require(reference_mhz == 12 and integer('CLKI_DIV') == 1, 'phase detector must be 12 MHz')
    require(parameters['FEEDBK_PATH'] == 'INT_OP', 'feedback must be internal CLKOP')
    require(400 <= vco_mhz <= 800, f'VCO {vco_mhz} MHz outside 400..800 MHz')
    require(parameters['DPHASE_SOURCE'] == 'DISABLED', 'dynamic phase must be disabled')
    for output_name in ('CLKOS', 'CLKOS2'):
        require(parameters[output_name + '_ENABLE'] == 'ENABLED', f'{output_name} disabled')
        require(integer(output_name + '_CPHASE') == 0 and integer(output_name + '_FPHASE') == 0,
                f'{output_name} phase must be zero')
        require(abs(vco_mhz / integer(output_name + '_DIV')
                    - float(attributes['FREQUENCY_PIN_' + output_name])) < 1e-9,
                f'{output_name} frequency is not an exact VCO division')
    template = {
        'reference_clock_port': 'clk',
        'output_clock_port': 'clk_cpu',
        'locked_port': 'pll_locked',
        'fabric_output': 'CLKOS',
        'additional_output_clock_ports': {'clk_mem': 'CLKOS2'},
        'feedback_output': 'CLKINTFB',
        'parameters': parameters,
        'attributes': attributes,
    }
    rendered = json.dumps(template, indent=2) + '\n'
    output.write_text(rendered)
    calculator = hashlib.sha256(ECPPLL.read_bytes()).hexdigest()
    help_output = subprocess.run([str(ECPPLL), '--help'], capture_output=True, text=True)
    version = help_output.stdout + help_output.stderr
    provenance.write_text(json.dumps({
        'command': ['ecppll', *arguments],
        'calculator_version': re.search(r'Version (\S+)', version)[1],
        'calculator_sha256': calculator,
        'generated_verilog_sha256': hashlib.sha256(text.encode()).hexdigest(),
        'binding_sha256': hashlib.sha256(rendered.encode()).hexdigest(),
        'vco_hz': round(vco_mhz * 1_000_000),
        'scope': 'Generator-produced zero CPHASE/FPHASE on CLKOS and CLKOS2; '
                 'CLKOP feedback keeps its generated phase.',
    }, indent=2) + '\n')
    print(output, f'VCO {vco_mhz:g} MHz')


if __name__ == '__main__':
    main()
