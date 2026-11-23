function g = gradeObjective(kind, limits, m, mRef, k)
% gradeObjective grades one objective value m after Tan 2017: grade = 10 - k*d.
%
%   kind    'lower' | 'upper' | 'range' | 'target' | 'minimize' | 'maximize'
%   limits  bound, [lower upper] or target; empty for a goal
%   m       value of the design, mRef the value of the reference design
%   k       deduction factor
%
% d is the miss relative to a scale s. A requirement misses by the distance to
% its bound or target and never goes below 0. A goal is d = (m - mRef)/|mRef|
% for minimize and the opposite for maximize, so it is negative when better
% than the reference. s falls back from |bound or target| to |mRef| to 1 when
% it is about 0; g.rule says which one was used.
switch kind
    case 'lower'
        bound = limits(1);
        miss = max(bound - m, 0);
    case 'upper'
        bound = limits(1);
        miss = max(m - bound, 0);
    case 'range'
        if m > limits(2)
            bound = limits(2);
            miss = m - bound;
        else
            bound = limits(1);
            miss = max(bound - m, 0);
        end
    case 'target'
        bound = limits(1);
        miss = abs(m - bound);
    case 'minimize'
        bound = mRef;
        miss = m - mRef;
    case 'maximize'
        bound = mRef;
        miss = mRef - m;
    otherwise
        error('mks:problem', 'Unknown objective kind %s', kind);
end

if ismember(kind, ["minimize" "maximize"])
    candidates = [abs(mRef) 1];
    rules = ["reference" "unit"];
else
    candidates = [abs(bound) abs(mRef) 1];
    rules = ["requirement" "reference" "unit"];
end
pick = find(candidates >= 1e-9, 1);
g.scale = candidates(pick);
g.rule = rules(pick);
g.d = miss / g.scale;
g.grade = 10 - k * g.d;
end
