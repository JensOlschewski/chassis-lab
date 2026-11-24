function historyStruct = run_mks_solver(jsonFile, designDic)
%==========================================================================
% run_mks_solver
%
% Description:
%   This function runs the MKS (Multibody Kinematic Solver) simulation
%   based on the input JSON file and an optional design vector. The function
%   returns the solution in format 2: bodies and joints addressed by the names
%   of the model JSON.
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
% Output:
%   - historyStruct (struct) : The solution, format 2. 3xn and 6xn blocks are
%                              split by meaning, all over the n time steps:
%
%       - format (1x1)  : 2
%       - time (1xn)    : Simulation time vector.
%       - bodies.<name>.velocity.{linear,angular}         : Body velocity (3xn each).
%       - bodies.<name>.cm.position                       : Center of mass (3xn).
%       - bodies.<name>.points.<point>.{position,velocity.{linear,angular}}
%       - joints.<name> : type, group ('primary' or 'secondary'), bushing
%                         (true when an active bushing sits on the joint), and
%                         the sides i and j with body, point, position,
%                         velocity.{linear,angular},
%                         reaction.{force,moment} and, in elastokinematic
%                         mode, reaction_gradient.{force,moment}.
%                         The reaction is the joint reaction plus the bushing force.
%       - characteristics : Suspension characteristics.
%
% Example:
%   history = run_mks_solver('input_config.json', []);
%   angular = history.bodies.B2_Wheelcarrier.velocity.angular;
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
