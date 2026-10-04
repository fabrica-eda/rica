"""Conservative capture boundaries in the presence of combinational hazards."""


def indices(observations):
    old=lambda point:bool(point['old_value_correct']) and all(n==point['trials_each'] for n in point['old_value_correct'])
    correct=lambda point:all(n==0 for n in point['errors'])
    last_old=None
    for j,point in enumerate(observations):
        if not old(point):break
        last_old=j
    first_zero=next((j for j,point in enumerate(observations) if correct(point)),None)
    last_error=max((j for j,point in enumerate(observations) if not correct(point)),default=-1)
    first_settled=last_error+1 if last_error+1<len(observations) else None
    return dict(last_old=last_old,first_zero=first_zero,first_settled=first_settled,
                intermediate_pass_window=first_zero!=first_settled,
                old_value_returned_after_departure=any(old(p) for p in observations[(last_old+1 if last_old is not None else 0):]))
