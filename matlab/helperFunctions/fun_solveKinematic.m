function historyStruct = fun_solveKinematic(jointPrimStruct, jointSecStruct, forceAddStruct, solverStruct, driveStruct)
% ##################################################################################
% Function: fun_solveKinematic
%
% Description:
%   Performs iterative kinematic analysis for a mechanical system over a specified
%   number of time steps. The function updates joint parameters, calculates forces,
%   solves for kinematic variables, and stores the history of velocities and forces.
%
% Inputs:
%   jointPrimStruct - Structure array containing primary joint parameters.
%   jointSecStruct  - Structure array containing secondary joint parameters.
%   forceAddStruct  - Structure array containing additional force elements (e.g., bushings).
%   solverStruct    - Structure containing solver settings and parameters, including:
%                     - steps: Number of iteration steps.
%                     - time_array: Array of time points.
%                     - other necessary solver matrices and parameters.
%   driveStruct     - Structure containing driving forces and parameters, including:
%                     - driveForceStruct: Structure with driving force signals.
%                     - driveReloStruct: Structure with drive-related vectors.
%
% Output:
%   historyStruct   - Structure storing velocity and force history alongside the time array.
%                     Fields include:
%                     - time: Array of time points.
%                     - v_hat: Cell array of generalized velocities at each time step.
%                     - fpi, fpj, fsi, fsj: Cell arrays of force components at each time step.
% ##################################################################################

% Extract solver parameters
steps = solverStruct.steps;             % Number of iteration steps
time_array = solverStruct.time_array;   % Time array
numB = solverStruct.numB;               % Number of bodies

% Number of bodies x 6 corresponds to length of v_hat
lengthVHat = solverStruct.numB * 6;

% Initialize history structures
historyStruct.time = time_array;
historyStruct.v_hat = zeros(lengthVHat, steps);
historyStruct.fpi = zeros(6*solverStruct.numPrimJoints, steps);
historyStruct.fpj = zeros(6*solverStruct.numPrimJoints, steps);
historyStruct.fsi = zeros(6*solverStruct.numSecJoints, steps);
historyStruct.fsj = zeros(6*solverStruct.numSecJoints, steps);

% -->>> Main solver iteration loop <<< ---
for i = 1:steps

    % Update progress bar
    ProgressBar(i, steps, time_array(i));

    % Update solver structure for current step
    solverStruct = createSolverStruct(solverStruct, 'Kinematic', jointPrimStruct, jointSecStruct);

    % Calculate forces from additional force elements
    feAdd = forceFromDeflection(forceAddStruct, numB);

    % Aggregate driving forces and additional forces
    fe = driveStruct.driveForceStruct.signalVec(:, i) + feAdd;

    % Solve for kinematic variables: beta (joint velocities) and lambda (Lagrange multipliers)
    gammaEll = solverStruct.Gs * (-driveStruct.driveReloStruct.driveVec(:, i));
    rhs = [-gammaEll; -fe];
    resultVector = solverStruct.solverMatrix \ rhs;

    % Extract beta and lambda from the result vector
    beta = resultVector(1:solverStruct.beta_length);
    lambda = resultVector(solverStruct.beta_length + 1:end);

    % Compute velocities at CMs
    v_hat = solverStruct.J * beta;

    % Compute joint forces
    [fpi, fpj, fsi, fsj] = computeJointForces(...
        solverStruct.Gpi, solverStruct.Gpj, ...
        solverStruct.Gsi, solverStruct.Gsj, ...
        solverStruct.Tpfi, solverStruct.Tpfj, ...
        solverStruct.Tsfi, solverStruct.Tsfj, ...
        lambda...
        );

    % Store results in history structure
    historyStruct.v_hat(:, i) = v_hat;
    historyStruct.fpi(:, i) = fpi;
    historyStruct.fpj(:, i) = fpj;
    historyStruct.fsi(:, i) = fsi;
    historyStruct.fsj(:, i) = fsj;

    jointPrimStruct = updateConParam(jointPrimStruct, v_hat, [], solverStruct.h,'kinematic');
    jointSecStruct  = updateConParam(jointSecStruct,  v_hat, [], solverStruct.h,'kinematic');
    forceAddStruct  = updateConParam(forceAddStruct,  v_hat, [], solverStruct.h,'kinematic');

    % Reinitialize parameters for the next iteration
    jointPrimStruct = fun_createJointParam(jointPrimStruct);
    jointSecStruct  = fun_createJointParam(jointSecStruct);
    forceAddStruct  = fun_createConParam(forceAddStruct);
end
end
