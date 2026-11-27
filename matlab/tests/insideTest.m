% Run from matlab/: matlab -batch "addpath(genpath('.')); insideTest"
% a 10 mm cube from 6 tets, written as HyperMesh ASCII (layout of the HyperMesh export)
f = [tempname '.hmascii'];
c = [0 0 0;10 0 0;10 10 0;0 10 0;0 0 10;10 0 10;10 10 10;0 10 10];
t = [1 2 3 7;1 3 4 7;1 4 8 7;1 8 5 7;1 5 6 7;1 6 2 7];
fid = fopen(f, 'w');
for k = 1:8, fprintf(fid, '*node(%d,%g,%g,%g,0,0)\n', k, c(k,:)); end
for k = 1:6, fprintf(fid, '*tetra4(%d,1,%d,%d,%d,%d,0)\n', k, t(k,:)); end
fclose(fid);
vol = readHmascii(f, 'cube');
near = @(a, b) abs(a - b) < 1e-9;
assert(near(inside(vol, [5;5;5]), 5));            % centre: 5 mm deep
assert(near(inside(vol, [10;5;5]), 0));           % on the surface
assert(near(inside(vol, [13;5;5]), -3));          % outside
assert(near(inside(vol, [13;13;5]), -sqrt(18)));  % beyond an edge
assert(near(inside(vol, [5;5;5], [13;5;5]), -3)); % several points: worst
assert(near(inside(vol, [5 5 12; 5 5 5; 5 5 5]), -2)); % 3 x n: worst step
% a point outside moves toward the volume: depth rises
assert(inside(vol, [12;5;5]) > inside(vol, [15;5;5]));
% errors name the file
g = [tempname '.hmascii']; fid = fopen(g, 'w'); fprintf(fid, '*node(1,0,0,0)\n'); fclose(fid);
try, readHmascii(g, 'empty'); error('expected'); catch err, assert(contains(err.message, g) && contains(err.message, 'empty')); end
disp('insideTest ok');
