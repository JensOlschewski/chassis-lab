function historyStruct = run_mks_solver(jsonFile, designDic, include)
%==========================================================================
% run_mks_solver
%
% Description:
%   This function runs the MKS (Multibody Kinematic Solver) simulation
%   based on the input JSON file and an optional design vector. The function
%   returns a structured output containing the simulation results, including
%   velocities, force gradients, and forces for primary and secondary joints,
%   along with the simulation time.
%
% Syntax:
%   historyStruct = run_mks_solver(jsonFile, designVec)
%
% Input:
%   - jsonFile (string) : Path to the input file in JSON format. This file
%                         contains the configuration and parameters required
%                         to run the MKS simulation.
%
%   - designVec (numeric array) : [Optional] Design Vector containing variable
%                                 design parameters for optimization purposes.
%                                 This vector allows adjusting specific parameters
%                                 to influence the behavior of the system.
%
%   - include (string array) : [Optional] Outputs to include: "bodies",
%                              "connectors", "suspension". When given, it
%                              replaces the include_* switches of the JSON.
%
% Output:
%   - historyStruct (struct) : A structure containing the results of the MKS
%                              simulation with the following fields:
%
%       - v_hat (nx3 double array) : Velocity vector of all bodies in the system,
%                                    where n is the number of bodies.
%
%       - dfpi, dfpj (nx3 double arrays) : Force gradient for the primary joints
%                                          or bushings on the i and j sides, respectively.
%
%       - dfspi, dfspj (nx3 double arrays) : Force gradient for the secondary joints
%                                            or bushings on the i and j sides, respectively.
%
%       - fpi, fpj (nx3 double arrays) : Force applied at the primary joints or bushings
%                                        on the i and j sides, respectively.
%
%       - fsi, fsj (nx3 double arrays) : Force applied at the secondary joints or bushings
%                                        on the i and j sides, respectively.
%
%       - time (1xn double array) : Simulation time vector, representing the time steps
%                                   at which the simulation results are recorded.
%
% Example:
%   jsonFile = 'input_config.json';
%   designVec = [1.2, 0.8, 1.5, ...];  % Example design parameters
%   history = run_mks_solver(jsonFile, designVec);
%
%   % Access the velocity vector and time
%   velocities = history.v_hat;
%   simulation_time = history.time;
%
% Notes:
%   - The input JSON file must follow the correct format, including all
%     required parameters for running the MKS simulation.
%   - The design vector is optional and used for optimization purposes.
%     If not provided, the function will run with default parameter settings defined in json.
%
% See also: jsondecode, some_other_function  % Add related functions if needed
%
% Author:
%   Jens Olschewski (jensolschewski@me.com)
%==========================================================================

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% Determine if DesignDic is provided
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
if nargin >= 2 && ~isempty(designDic)
    % designDic is provided, JSON will be updated with the provided values
    disp('Design dictionary provided. The JSON structure will be updated with the provided design parameters.');
else
    designDic = [];
    % designDic is not provided, default values will be used
    % disp('No design dictionary provided. The default JSON structure will be used.');
end

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% READ JSON
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
jsonStr = fileread(jsonFile);

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% ADD Helper Function Path
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% addpath('./helperFunctions');

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% Initialization
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
[jointPrim, jointSec, bushPrim, bushSec, forceAdd, bodiesStruct, solverStruct, driveStruct,outputStruct] = solver_init(jsonStr, designDic);

% Optional include list overrides the output switches of the model JSON
if nargin >= 3
    outputStruct.isBodyOutput = ismember("bodies", include);
    outputStruct.isConOutput = ismember("connectors", include);
    outputStruct.isSuspensionParamOutput = ismember("suspension", include);
end

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% Preload & Main Processing
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
if solverStruct.isKin
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    % Kinematic mode
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    historyStruct = fun_solveKinematic(jointPrim, jointSec, forceAdd, solverStruct, driveStruct);

else
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    % Preload elastokinematic mode
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    % if driveStruct.ispreload || any([bushPrim.isPreload]) || any([bushSec.isPreload]) || any([forceAdd.isPreload])
    %     [historyStruct, jointPrim, jointSec, bushPrim, bushSec, forceAdd] = fun_solvePreloadCompliant(jointPrim, jointSec, bushPrim, bushSec, forceAdd, driveStruct, solverStruct);
    % end
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    % Elastokinematic mode
    %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
    historyStruct = fun_solveCompliant(jointPrim, jointSec, bushPrim, bushSec, forceAdd, solverStruct, driveStruct);
end

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
% Post-processing
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
historyStruct = postProcess(historyStruct, bodiesStruct, jointPrim, jointSec, bushPrim, bushSec, outputStruct);
end
