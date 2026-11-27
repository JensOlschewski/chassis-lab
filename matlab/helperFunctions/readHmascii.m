function vol = readHmascii(file, key)
% readHmascii reads a HyperMesh ASCII tet mesh (*node, *tetra4, *tetra10).
% vol.nodes is n x 3, vol.tets m x 4 (row indices; tet10: corner nodes only),
% vol.TR the triangulation, vol.F / vol.V the boundary triangles and points.
% Unverified against real HyperMesh files: node = (id,x,y,z,...), tet = (id,...,n1..n4)
% with the nodes as the last four ids for tetra4 and the first four of the node list for tetra10.
text = fileread(file);
nodeLines = regexp(text, '^\*node\(([^)]*)\)', 'tokens', 'lineanchors');
tetLines = regexp(text, '^\*tetra(4|10)\(([^)]*)\)', 'tokens', 'lineanchors');
if isempty(nodeLines)
    error('mks:volume', 'Volume %s (%s) has no *node lines', key, file);
end
if isempty(tetLines)
    error('mks:volume', 'Volume %s (%s) has no *tetra4 or *tetra10 lines', key, file);
end
ids = zeros(numel(nodeLines), 1);
nodes = zeros(numel(nodeLines), 3);
for k = 1:numel(nodeLines)
    f = sscanf(strrep(nodeLines{k}{1}, ',', ' '), '%f')';
    if numel(f) < 4
        error('mks:volume', 'Volume %s (%s): bad *node line %d', key, file, k);
    end
    ids(k) = f(1);
    nodes(k, :) = f(2:4);
end
tets = zeros(numel(tetLines), 4);
for k = 1:numel(tetLines)
    f = sscanf(strrep(tetLines{k}{2}, ',', ' '), '%f')';
    if strcmp(tetLines{k}{1}, '4')
        c = f(end-3:end);
    else
        c = f(end-9:end-6);
    end
    [found, at] = ismember(c, ids);
    if numel(c) ~= 4 || ~all(found)
        error('mks:volume', 'Volume %s (%s): *tetra line %d uses an unknown node', key, file, k);
    end
    tets(k, :) = at;
end
vol.nodes = nodes;
vol.tets = tets;
vol.TR = triangulation(tets, nodes);
[vol.F, vol.V] = freeBoundary(vol.TR);
end
