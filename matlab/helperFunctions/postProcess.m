function historyStruct = postProcess(historyStruct, bodiesStruct, jointPrimStruct, jointSecStruct, bushPrimStruct, bushSecStruct, outputStruct)
% ##################################################################################
% Function: postProcess
% Description: Processes history and bodies structures to compute the kinematics
%              (velocity and position) of each body and its points. For each body,
%              it extracts the center of mass velocity from the history and integrates
%              to get the position. It also calculates the rigid body movement of
%              additional points on each body.
%
% Inputs:
%   historyStruct - Structure containing historical data for each time step, including
%                   velocities for each body.
%   bodiesStruct  - Structure defining the properties of each body (e.g., name, ID, sides).
%   bushPrimStruct     - Structure containing primary bushing data.
%   bushSecStruct      - Structure containing additional bushing data.
%   outputStruct - Structure containing settings or parameters for post-processing.
%
% Output:
%   historyStruct - Updated structure containing calculated velocities and positions
%                   for the center of mass and other points on each body.
% ##################################################################################

% Time step for integration
timeArray = historyStruct.time;
h = diff(historyStruct.time(1:2));

% Output for Bodies
if outputStruct.isBodyOutput ||  outputStruct.isSuspensionParamOutput
    % Process bodies using helper function
    historyStruct.bodies = processBodies(bodiesStruct, historyStruct, timeArray, h);
end

% Output for connectors
if outputStruct.isConOutput
    % Initialize joint and bushing structures
    joints = struct();
    bushings = struct();

    % Function for process connector output
    [joints.primary, bushings.primary] = processConnectors(jointPrimStruct, bushPrimStruct, historyStruct, timeArray, h);
    [joints.secondary, bushings.secondary] = processConnectors(jointSecStruct, bushSecStruct, historyStruct, timeArray, h);
    historyStruct.joints = joints;
    historyStruct.bushings = bushings;
end


% Output for suspension parameters
if outputStruct.isSuspensionParamOutput
    % Initialize characteristics structure
    characteristics = struct();

    % Define wheel sides and prefixes
    wheelSides = {'Left', 'Right'};
    sidePrefixes = {'l', 'r'};

    % Process each side
    for i = 1:2
        side = wheelSides{i};
        prefix = sidePrefixes{i};

        % Extract wheel points
        [WC, WN, WP] = extractWheelPoints(side, outputStruct, historyStruct);
        characteristics.wheel_points.([prefix '_wc']) = WC;
        characteristics.wheel_points.([prefix '_wn']) = WN;
        characteristics.wheel_points.([prefix '_wp']) = WP;

        % Calculate wheel velocities
        characteristics.wheel_velocities = computeVelocities(characteristics.wheel_points, timeArray);

        % Calculate toe and camber
        toe = rad2deg(calculateToe(WC, WN));
        camber = rad2deg(calculateCamber(WC, WN));
        characteristics.toecamber.([prefix '_toe']) = toe;
        characteristics.toecamber.([prefix '_camber']) = camber;

        % Get wheel patch information
        wheelPatchInfo = getWheelPatchInformation(side, outputStruct, jointPrimStruct, jointSecStruct);

        % Extract platform movement
        platformPosition = historyStruct.bodies(wheelPatchInfo.PlattformBodyID).points.(wheelPatchInfo.plattform_pointname).position;
        characteristics.platform.([prefix '_platform_position']) = platformPosition;
        characteristics.platform.([prefix '_tire_deflection']) = WP(3, :) - platformPosition(3, :);

        % Calculate platform velocities 
        characteristics.platform_velocities = computeVelocities(characteristics.platform, timeArray);
       
        % Calculate wheel load
        currentJointIndices = (wheelPatchInfo.wheelPatchJointID * 6 - 3);
        wheelload = historyStruct.([wheelPatchInfo.reactionPrefix wheelPatchInfo.suffix])(currentJointIndices, :);
        characteristics.loads.([prefix '_wheel_load']) = wheelload;

        % Calculate ride and wheel rate
        characteristics.rates.([prefix '_ride_rate']) = gradient(wheelload,platformPosition(3, :));
        characteristics.rates.([prefix '_wheel_rate']) = gradient(wheelload,WC(3, :));

        % % Extract Spring and Damper Motions
        % if outputStruct.(['spring' side]).suBodyID == 0
        %    suBodyID = length(historyStruct.bodies);
        %    sidePrefix = 'ls'
        % end
        %
        % if outputStruct.(['spring' side]).slBodyID == 0
        %    slBodyID = length(historyStruct.bodies);
        % end

        % SU = historyStruct.bodies(suBodyID).points.(outputStruct.(['spring' side]).suName).position;


        % Calculate Spring and Damper Ratios

        % extractSpringPoints(side, outputStruct, historyStruct);

    end
    % Calculate Rollangle
    z_disp = characteristics.platform.r_platform_position(3,:) -  characteristics.platform.l_platform_position(3,:);
    y_disp = characteristics.platform.r_platform_position(2,:) -  characteristics.platform.l_platform_position(2,:);
    characteristics.roll.rollangle = atan(z_disp./y_disp);

    % Calculate Rollrate
    left_tire_rollmoment = characteristics.loads.l_wheel_load .* characteristics.wheel_points.l_wp(2,:);
    right_tire_rollmoment = characteristics.loads.r_wheel_load .* characteristics.wheel_points.r_wp(2,:);
    characteristics.roll.rollmoment = left_tire_rollmoment + right_tire_rollmoment;
    characteristics.roll.roll_rate = gradient(characteristics.roll.rollmoment, characteristics.roll.rollangle);

    % Calculate Rollcenterposition (PHD Defintion)
    alpha_ast = abs(atan(characteristics.wheel_velocities.l_wp(2,:)./characteristics.platform_velocities.l_platform_position(3,:))); 
    beta_ast = abs(atan(characteristics.wheel_velocities.r_wp(2,:)./characteristics.platform_velocities.r_platform_position(3,:))); 
    alpha = alpha_ast - characteristics.roll.rollangle;
    beta = beta_ast + characteristics.roll.rollangle;
    b = characteristics.wheel_points.r_wp(2,:) - characteristics.wheel_points.l_wp(2,:);
    characteristics.roll.rollcenter_height = b .* (sin(alpha) .* sin(beta))./ sin(alpha + beta);
    characteristics.roll.rollcenter_ypos = b./2  - characteristics.roll.rollcenter_height ./ tan(alpha);

    % Store characteristics in historyStruct
    historyStruct.characteristics = characteristics;
end

% The suspension parameters need the bodies; they are only output on request
if ~outputStruct.isBodyOutput && isfield(historyStruct, 'bodies')
    historyStruct = rmfield(historyStruct, 'bodies');
end

end


% #####################
% Helper Functions
% #####################

function bodies = processBodies(bodiesStruct, historyStruct, timeArray, h)

% Extract number of bodies
numBodies = numel(bodiesStruct);

% Predefine the structure fields for bodies array
bodies = repmat(struct('name', [], 'bodyID', [], 'cm', struct(), 'points', struct()), numBodies, 1);

% Loop over all bodies in bodiesStruct
for i = 1:numBodies

    % Extract current body struct
    currentBodyStruct = bodiesStruct(i);

    % Get point names and initial positions
    pointNames = fieldnames(currentBodyStruct.points);
    initialPositions = currentBodyStruct.points;

    % Process body
    bodies(i) = processEntity(currentBodyStruct, historyStruct, timeArray, h, pointNames, initialPositions);
end
end


function [joints,bushings] = processConnectors(jointStruct, bushStruct, historyStruct, timeArray, h)

% Extract number of joints
numJoints = numel(jointStruct);

% Predefine the structure fields for joint Struct
joints = struct();
bushings = struct();

% Loop over all joints in jointStruct
for idx = 1:numJoints

    % Determine 'conMode' (joint or bushing)
    conMode = 'joint';
    if idx <= numel(bushStruct) && bushStruct(idx).active && ismember(bushStruct(idx).jointID, [jointStruct.jointID])
        conMode = 'bushing';
    end

    switch conMode
        case 'joint'
            currentJointStruct = jointStruct(idx);

            % Current topology
            currentJointTopology = currentJointStruct.topology;

            % Write basic information
            joints(idx).name = currentJointStruct.name;
            joints(idx).jointID = currentJointStruct.jointID;
            joints(idx).type = currentJointStruct.type;

            sidesName = {'i', 'j'};

            % Iterate through sides
            for k = 1:2
                sideName = sidesName{k};
                currentSideStruct = currentJointStruct.(sideName);
                bodyID = currentSideStruct.bodyID;

                joints(idx).(sideName).name = currentSideStruct.pointName;
                joints(idx).(sideName).bodyID = bodyID;

                % Process side
                initialPosition = currentSideStruct.point;

                if bodyID == 0
                    % Ground
                    numSteps = length(timeArray);
                    joints(idx).(sideName).velocity = zeros(6, numSteps);
                    joints(idx).(sideName).position = repmat(initialPosition, 1, numSteps);
                else
                    % Extract v_hat for the body
                    bodyIndices = (bodyID * 6 - 5):(bodyID * 6);
                    vCmArray = historyStruct.v_hat(bodyIndices, :);

                    % s vector
                    s = currentSideStruct.s;

                    % Calculate velocities and positions
                    [velocity, position] = processSinglePoint(vCmArray, s, initialPosition, timeArray, h);

                    joints(idx).(sideName).velocity = velocity;
                    joints(idx).(sideName).position = position;
                end

                % Extract reaction forces
                if strcmp(currentJointTopology,"primary")
                    reactionName = ['fp', sideName];
                elseif strcmp(currentJointTopology,"secondary")
                    reactionName = ['fs', sideName];
                end
                reactionIndices = (currentJointStruct.jointID * 6 - 5):(currentJointStruct.jointID * 6);
                joints(idx).(sideName).reactions = historyStruct.(reactionName)(reactionIndices, :);
            end

        case 'bushing'

            % Process bushing

    end
end
end

function entity = processEntity(entityStruct, historyStruct, timeArray, h, pointFields, initialPositions)

% Initialize entity properties
entity = struct();
entity.name = entityStruct.name;
entity.bodyID = entityStruct.bodyID;
numSteps = length(timeArray);

if entity.bodyID ~= 0
    % Extract center of mass velocity from historyStruct
    velocityIndices = (entity.bodyID * 6 - 5):(entity.bodyID * 6);
    entity.cm.velocity = historyStruct.v_hat(velocityIndices, :);

    % Integrate center of mass position using cumtrapz
    entity.cm.position = cumtrapz(timeArray, entity.cm.velocity(1:3, :)')' + entityStruct.cm;

    % Process points or sides
    entity.points = processPoints(entity, timeArray, h, pointFields, initialPositions);
else
    entity.cm.velocity = zeros(6, numSteps);
    entity.cm.position = zeros(3, numSteps);

    % Initialize points with zeros
    for j = 1:length(pointFields)
        pointName = pointFields{j};
        entity.points.(pointName).velocity = zeros(6, numSteps);
        entity.points.(pointName).position = repmat(initialPositions.(pointName), 1, numSteps);
    end
end
end

function points = processPoints(entity, timeArray, h, pointNames, initialPositions)
numSteps = length(timeArray);

for j = 1:length(pointNames)
    pointName = pointNames{j};
    points.(pointName).velocity = zeros(6, numSteps);
    points.(pointName).position = zeros(3, numSteps);

    % Initial vector s from the CM to the point
    s = initialPositions.(pointName) - entity.cm.position(:, 1);

    for k = 1:numSteps
        vCM = entity.cm.velocity(:, k);
        points.(pointName).velocity(:, k) = calculateVelocity(vCM, s);

        if k < numSteps
            R = createRotFromAngVel(vCM(4:6), h(1), 'rodrigues');
            s = R * s;
        end
    end

    % Integrate velocity to calculate position
    points.(pointName).position = cumtrapz(timeArray, points.(pointName).velocity(1:3, :)')' + initialPositions.(pointName);
end
end

function [velocity, position] = processSinglePoint(vCmArray, s, initialPosition, timeArray, h)
numSteps = length(timeArray);
velocity = zeros(6, numSteps);
sArray = zeros(3, numSteps);
sArray(:, 1) = s;

for k = 1:numSteps
    vCM = vCmArray(:, k);
    velocity(:, k) = calculateVelocity(vCM, sArray(:, k));

    if k < numSteps
        R = createRotFromAngVel(vCM(4:6), h(1), 'rodrigues');
        sArray(:, k+1) = R * sArray(:, k);
    end
end

% Integrate velocity to get position
position = cumtrapz(timeArray, velocity(1:3, :)')' + initialPosition;
end

function velocity = calculateVelocity(vCM, r)
velocity = [vCM(1:3) + cross(vCM(4:6), r); vCM(4:6)];
end

function toeAngle = calculateToe(wc, wn)
deltaX = wc(1, :) - wn(1, :);
deltaY = wc(2, :) - wn(2, :);
toeAngle = atan(deltaX ./ sqrt(deltaX.^2 + deltaY.^2));
end

function camberAngle = calculateCamber(wc, wn)
deltaX =wc(1, :) - wn(1, :);
deltaY =wc(2, :) - wn(2, :);
deltaZ =wc(3, :) - wn(3, :);
camberAngle = atan(deltaZ ./ sqrt(deltaX.^2 + deltaY.^2 + deltaZ.^2));
end

function [WC, WN, WP] = extractWheelPoints(side, outputStruct, historyStruct)
% Retrieve wheel data
wheelData = outputStruct.(['wheel' side]);
WheelBodyID = wheelData.bodyID;

% Construct point names
wcPointName = strcat(lower(wheelData.wcName), num2str(WheelBodyID));
wnPointName = strcat(lower(wheelData.wnName), num2str(WheelBodyID));
wpPointName = strcat(lower(wheelData.wpName), num2str(WheelBodyID));

% Extract positions
WC = historyStruct.bodies(WheelBodyID).points.(wcPointName).position;
WN = historyStruct.bodies(WheelBodyID).points.(wnPointName).position;
WP = historyStruct.bodies(WheelBodyID).points.(wpPointName).position;
end

function wheelPatchInfo = getWheelPatchInformation(side, outputStruct, jointPrimStruct, jointSecStruct)
% Retrieve wheel data
wheelData = outputStruct.(['wheel' side]);
wheelPatchInfo.WheelBodyID = wheelData.bodyID;
wheelPatchInfo.wheelPatchJointID = wheelData.jointID;
currentJointTopology = wheelData.jointTopology;

% Determine the correct joint structure and reaction prefix
if strcmp(currentJointTopology, 'primary')
    wheelPatchInfo.currentWheelPatchJointStruct = jointPrimStruct(wheelPatchInfo.wheelPatchJointID);
    wheelPatchInfo.reactionPrefix = 'fp';
else
    wheelPatchInfo.currentWheelPatchJointStruct = jointSecStruct(wheelPatchInfo.wheelPatchJointID);
    wheelPatchInfo.reactionPrefix = 'fs';
end

% Check connection to the wheel body and determine suffix
if wheelPatchInfo.currentWheelPatchJointStruct.i.bodyID == wheelPatchInfo.WheelBodyID
    wheelPatchInfo.suffix = 'i';
    wheelPatchInfo.PlattformBodyID = wheelPatchInfo.currentWheelPatchJointStruct.j.bodyID;
    wheelPatchInfo.plattform_pointname = wheelPatchInfo.currentWheelPatchJointStruct.j.pointName;
elseif wheelPatchInfo.currentWheelPatchJointStruct.j.bodyID == wheelPatchInfo.WheelBodyID
    wheelPatchInfo.suffix = 'j';
    wheelPatchInfo.PlattformBodyID = wheelPatchInfo.currentWheelPatchJointStruct.i.bodyID;
    wheelPatchInfo.plattform_pointname = wheelPatchInfo.currentWheelPatchJointStruct.i.pointName;
else
    error('Wheel patch joint is not connected to the wheel body.');
end
end

function velocities = computeVelocities(pointStruct, timeArray)
    fieldsList = fieldnames(pointStruct);
    velocities = struct();
    
    for k = 1:numel(fieldsList)
        data = pointStruct.(fieldsList{k});
        dData = zeros(size(data));
        for row = 1:size(data, 1)
            dData(row, :) = gradient(data(row, :), timeArray);
        end
        velocities.(fieldsList{k}) = dData;
    end
end