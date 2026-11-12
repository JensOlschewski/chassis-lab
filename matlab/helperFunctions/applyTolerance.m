function Val = applyTolerance(Val, tolerance)
    Val(abs(Val) < tolerance) = 0;
end