function depth = inside(vol, varargin)
% inside depth in mm of the points (3 x n each, columns = time steps) in the
% volume: positive inside, negative outside, the smallest over all points.
depth = Inf;
for k = 1:numel(varargin)
    P = varargin{k};
    if size(P, 1) ~= 3
        error('mks:volume', 'inside needs points as 3 x n');
    end
    inTet = ~isnan(pointLocation(vol.TR, P'));
    d = surfaceDistance(vol.V, vol.F, P);
    depth = min(depth, min(d(:)' .* (2 * inTet(:)' - 1)));
end
end

function d = surfaceDistance(V, F, P)
% distance of each point (column) to the nearest boundary triangle
A = V(F(:,1), :); B = V(F(:,2), :); C = V(F(:,3), :);
d = zeros(1, size(P, 2));
for k = 1:size(P, 2)
    p = P(:, k)';
    d(k) = sqrt(min(sqDistPointTriangle(p, A, B, C)));
end
end

function d2 = sqDistPointTriangle(p, A, B, C)
% squared distance to many triangles (Ericson, vectorised over triangles)
n = size(A, 1);
Q = zeros(n, 3);
ab = B - A; ac = C - A; ap = p - A;
d1 = dot(ab, ap, 2); d2_ = dot(ac, ap, 2);
bp = p - B; d3 = dot(ab, bp, 2); d4 = dot(ac, bp, 2);
cp = p - C; d5 = dot(ab, cp, 2); d6 = dot(ac, cp, 2);
vc = d1.*d4 - d3.*d2_; vb = d5.*d2_ - d1.*d6; va = d3.*d6 - d5.*d4;
% face region (default)
den = va + vb + vc; den(den == 0) = 1;
Q = A + ab .* (vb ./ den) + ac .* (vc ./ den);
% edge AB, AC, BC and vertices, overriding in the order Ericson tests them
m = vc <= 0 & d1 >= 0 & d3 <= 0; Q(m, :) = A(m, :) + ab(m, :) .* (d1(m) ./ max(d1(m) - d3(m), eps));
m = vb <= 0 & d2_ >= 0 & d6 <= 0; Q(m, :) = A(m, :) + ac(m, :) .* (d2_(m) ./ max(d2_(m) - d6(m), eps));
m = va <= 0 & (d4 - d3) >= 0 & (d5 - d6) >= 0;
w = (d4 - d3) ./ max((d4 - d3) + (d5 - d6), eps);
Q(m, :) = B(m, :) + (C(m, :) - B(m, :)) .* w(m);
m = d6 >= 0 & d5 <= d6; Q(m, :) = C(m, :);
m = d3 >= 0 & d4 <= d3; Q(m, :) = B(m, :);
m = d1 <= 0 & d2_ <= 0; Q(m, :) = A(m, :);
d2 = sum((p - Q).^2, 2);
end
