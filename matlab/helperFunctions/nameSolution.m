function solution = nameSolution(history)
% Builds solution format 2 from the post-processed history: bodies and joints
% are addressed by the names of the model JSON, 6-row blocks are split into
% linear/angular and force/moment. Ground is not a body entry. Everything stacked by position in the
% solver (v_hat, fpi, dfpi, ...) is dropped from the top level.

solution = struct();
solution.format = 2;
solution.time = history.time;

bodyIDs = [history.bodies.bodyID];
bodyNames = {history.bodies.name};

solution.bodies = struct();
for body = history.bodies(:)'
    if body.bodyID == 0
        continue    % ground never moves; joints name it in their "body" field
    end
    name = uniqueName(solution.bodies, body.name, 'body');
    entry = struct();
    entry.velocity = splitVelocity(body.cm.velocity);
    entry.cm.position = body.cm.position;
    entry.points = struct();
    for pointName = fieldnames(body.points)'
        point = body.points.(pointName{1});
        entry.points.(pointName{1}) = struct('position', point.position, ...
            'velocity', splitVelocity(point.velocity));
    end
    solution.bodies.(name) = entry;
end

solution.joints = struct();
for group = {'primary', 'secondary'}
    for joint = history.joints.(group{1})(:)'
        name = uniqueName(solution.joints, joint.name, 'joint');
        entry = struct('type', joint.type, 'group', group{1}, 'bushing', joint.bushing);
        for side = {'i', 'j'}
            data = joint.(side{1});
            sideEntry = struct();
            sideEntry.body = bodyNames{bodyIDs == data.bodyID};
            sideEntry.point = data.name;
            sideEntry.position = data.position;
            sideEntry.velocity = splitVelocity(data.velocity);
            sideEntry.reaction = splitReaction(data.reactions);
            if isfield(data, 'reactionGradients')
                sideEntry.reaction_gradient = splitReaction(data.reactionGradients);
            end
            entry.(side{1}) = sideEntry;
        end
        solution.joints.(name) = entry;
    end
end

if isfield(history, 'characteristics')
    solution.characteristics = history.characteristics;
end
end

function name = uniqueName(group, name, kind)
if ~isvarname(name)
    error('mks:name', 'The %s name "%s" is not a valid identifier.', kind, name);
end
if isfield(group, name)
    error('mks:name', 'The %s name "%s" is used twice.', kind, name);
end
end

function v = splitVelocity(rows)
v = struct('linear', rows(1:3, :), 'angular', rows(4:6, :));
end

function r = splitReaction(rows)
r = struct('force', rows(1:3, :), 'moment', rows(4:6, :));
end
