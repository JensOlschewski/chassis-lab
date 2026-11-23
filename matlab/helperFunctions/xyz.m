function component = xyz(vector, axis)
% xyz returns one component of a 3xn vector signal as a 1xn row.
if size(vector, 1) ~= 3
    error('mks:signal', 'A .x, .y or .z component needs a 3xn signal, got %dx%d', size(vector, 1), size(vector, 2));
end
component = vector(axis, :);
end
