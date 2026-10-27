"""Offline tests: never post a heartbeat or read credentials."""
import pathlib
import runpy
import unittest

agent = runpy.run_path(str(pathlib.Path(__file__).with_name('status-heartbeat.py')))
sample, rates = agent['network_sample'], agent['network_rates']
ROUTES = 'Iface Destination Gateway Flags RefCnt Use Metric Mask\nens3 00000000 0100000A 0003 0 0 0 00000000\nens3 00000000 0100000A 0003 0 0 1 00000000\nwg-mesh 00004D0A 00000000 0001 0 0 0 00FFFFFF'


def devices(rx, tx):
    return f'ens3: {rx} 0 0 0 0 0 0 0 {tx} 0 0 0 0 0 0 0\nlo: 99999999 0 0 0 0 0 0 0 99999999 0 0 0 0 0 0 0\nwg-mesh: 99999999 0 0 0 0 0 0 0 99999999 0 0 0 0 0 0 0'


class TrafficTest(unittest.TestCase):
    def test_wan_is_counted_once_and_rates_use_actual_interval(self):
        before, after = sample(ROUTES, devices(1000, 2000)), sample(ROUTES, devices(251000, 502000))
        self.assertEqual(set(before), {'ens3'})
        self.assertEqual(rates(before, after, 2), {'rxMbps': 1, 'txMbps': 2})
        self.assertEqual(rates(before, before, 1), {'rxMbps': 0, 'txMbps': 0})

    def test_missing_reset_and_changed_interface_mean_no_data(self):
        first = sample(ROUTES, devices(100, 200))
        missing = {'rxMbps': None, 'txMbps': None}
        self.assertIsNone(sample('', devices(100, 200)))
        self.assertIsNone(sample(ROUTES, 'ens3: broken'))
        self.assertEqual(rates(first, sample(ROUTES, devices(0, 100)), 1), missing)
        self.assertEqual(rates(first, {'ens4': (200, 400)}, 1), missing)
        self.assertEqual(rates(first, None, 1), missing)
        self.assertEqual(rates(first, first, 0), missing)


if __name__ == '__main__':
    unittest.main()
