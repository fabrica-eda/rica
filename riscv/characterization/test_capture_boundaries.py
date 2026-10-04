import unittest
from capture_boundaries import indices


def point(errors,old):
    return dict(errors=[errors,errors],old_value_correct=[old,old],trials_each=4)


class CaptureBoundaryTests(unittest.TestCase):
    def test_hazard_after_temporary_correct_window_uses_final_settling(self):
        result=indices([point(4,4),point(4,0),point(0,0),point(1,0),point(0,0),point(0,0)])
        self.assertEqual(result['last_old'],0)
        self.assertEqual(result['first_settled'],4)
        self.assertTrue(result['intermediate_pass_window'])

    def test_return_to_old_value_does_not_hide_earliest_transition(self):
        result=indices([point(4,4),point(4,0),point(4,4),point(0,0)])
        self.assertEqual(result['last_old'],0)
        self.assertEqual(result['first_settled'],3)
        self.assertTrue(result['old_value_returned_after_departure'])

    def test_unsettled_and_phase_insensitive_data_remain_censored(self):
        self.assertIsNone(indices([point(4,4),point(2,0)])['first_settled'])
        self.assertIsNone(indices([point(0,0),point(0,0)])['last_old'])


if __name__=='__main__':unittest.main()
