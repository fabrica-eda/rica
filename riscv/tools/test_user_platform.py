"""Platform gates must check the physical PLL, not just reported periods."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

from user_platform import PROJECT, check_checkpoint_clocks, generated, load_profile


class PlatformGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.profile = load_profile()
        self.path = Path(self.temp.name) / 'platform.json'
        self.path.write_text(json.dumps(self.profile))
        pll = json.loads(generated(self.profile)[PROJECT / self.profile['pll']])
        pll['kind'] = 'pll'
        periods = [(10**12 + self.profile[k] // 2) // self.profile[k]
                   for k in ('cpu_hz', 'memory_hz')]
        self.cp = {
            'primitive_metadata': [{'configuration': pll}],
            'packing': {'generated_clock_periods_ps': [
                {'period_ps': p} for p in [*periods, 166666]]},
            'timing': {'setup_uncertainty_ps': 194, 'hold_uncertainty_ps': 194},
        }

    def test_matching_physical_platform(self):
        self.assertEqual(check_checkpoint_clocks(self.cp, self.path), self.profile)

    def test_claimed_period_does_not_hide_wrong_pll(self):
        for field, value in [('CLKOS_DIV', '5'), ('CLKOS2_CPHASE', '1')]:
            with self.subTest(field=field):
                cp = copy.deepcopy(self.cp)
                cp['primitive_metadata'][0]['configuration']['parameters'][field] = value
                with self.assertRaisesRegex(ValueError, 'physical PLL parameters'):
                    check_checkpoint_clocks(cp, self.path)

    def test_guard_and_jtck_cannot_be_relaxed(self):
        for field in ('setup_uncertainty_ps', 'hold_uncertainty_ps'):
            cp = copy.deepcopy(self.cp)
            cp['timing'][field] = 0
            with self.assertRaisesRegex(ValueError, 'guard changed'):
                check_checkpoint_clocks(cp, self.path)
        self.cp['packing']['generated_clock_periods_ps'][-1]['period_ps'] = 200000
        with self.assertRaisesRegex(ValueError, 'clock periods'):
            check_checkpoint_clocks(self.cp, self.path)

    def test_invalid_profile_rejected(self):
        for field, value in [('memory_hz', 0), ('cpu_hz', 80_000_000),
                             ('setup_hold_guard_ps', 0), ('early_alu', 'false')]:
            with self.subTest(field=field):
                profile = {**self.profile, field: value}
                self.path.write_text(json.dumps(profile))
                with self.assertRaises(ValueError):
                    load_profile(self.path)

    def test_direct_load_return_requires_equal_clocks(self):
        self.profile.update(cpu_hz=78_000_000, memory_hz=39_000_000,
                            direct_load_return=True)
        self.path.write_text(json.dumps(self.profile))
        with self.assertRaises(ValueError):
            load_profile(self.path)

    def test_intermediate_half_rate_templates_keep_zero_phase(self):
        for vco, cpu, divider in [(648_000_000, 108_000_000, 6), (672_000_000, 112_000_000, 6)]:
            with self.subTest(cpu=cpu):
                self.profile.update(cpu_hz=cpu, memory_hz=cpu // 2, vco_hz=vco)
                self.path.write_text(json.dumps(self.profile))
                profile = load_profile(self.path)
                pll = json.loads(generated(profile)[PROJECT / profile['pll']])
                params = pll['parameters']
                self.assertEqual(params['CLKI_DIV'], '1')
                self.assertEqual(params['FEEDBK_PATH'], 'INT_OP')
                self.assertEqual(12 * int(params['CLKFB_DIV']) * int(params['CLKOP_DIV']),
                                 vco // 1_000_000)
                self.assertEqual(params['CLKOS_DIV'], str(divider))
                self.assertEqual(params['CLKOS2_DIV'], str(2 * divider))
                for pin in ('CLKOS', 'CLKOS2'):
                    self.assertEqual(params[pin + '_CPHASE'], '0')
                    self.assertEqual(params[pin + '_FPHASE'], '0')
                self.assertEqual(pll['attributes']['FREQUENCY_PIN_CLKOS'], f'{cpu // 1_000_000}')
                self.assertEqual(pll['attributes']['FREQUENCY_PIN_CLKOS2'], f'{cpu // 2_000_000}')

    def test_84_mhz_uses_exact_generated_pll_and_preserves_guards(self):
        self.profile.update(cpu_hz=84_000_000, memory_hz=84_000_000,
                            vco_hz=588_000_000)
        self.path.write_text(json.dumps(self.profile))
        profile = load_profile(self.path)
        pll = json.loads(generated(profile)[PROJECT / profile['pll']])
        self.assertEqual(pll['parameters']['CLKFB_DIV'], '7')
        self.assertEqual(pll['parameters']['CLKOP_DIV'], '7')
        self.assertEqual(pll['parameters']['CLKOS_DIV'], '7')
        self.assertEqual(pll['parameters']['CLKOS2_DIV'], '7')
        for pin in ('CLKOS', 'CLKOS2'):
            self.assertEqual(pll['parameters'][pin + '_CPHASE'], '0')
            self.assertEqual(pll['parameters'][pin + '_FPHASE'], '0')
        self.assertEqual(pll['attributes']['FREQUENCY_PIN_CLKOS'], '84')
        pll['kind'] = 'pll'
        self.cp['primitive_metadata'][0]['configuration'] = pll
        self.cp['packing']['generated_clock_periods_ps'] = [
            {'period_ps': value} for value in (11905, 11905, 166666)]
        self.assertEqual(check_checkpoint_clocks(self.cp, self.path), profile)
        self.cp['primitive_metadata'][0]['configuration']['parameters']['CLKFB_DIV'] = '1'
        with self.assertRaisesRegex(ValueError, 'physical PLL parameters'):
            check_checkpoint_clocks(self.cp, self.path)

    def test_unrecognized_vco_is_rejected(self):
        self.profile.update(cpu_hz=100_000_000, memory_hz=100_000_000,
                            vco_hz=600_000_000)
        self.path.write_text(json.dumps(self.profile))
        with self.assertRaises(ValueError):
            load_profile(self.path)


if __name__ == '__main__':
    unittest.main()
