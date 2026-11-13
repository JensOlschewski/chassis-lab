function [jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,forceAddStruct,bodiesStruct,solverStruct,driveStruct,outputStruct] = solver_init(jsonStr, designDic)
    % ##################################################################################
    % Function: solver_init
    % Description: Initializes structures for bodies, joints, bushings, forces, solver,
    %              ,drives and output based on the provided JSON input. Optionally applies a
    %              design dictionary mapping if provided.
    %
    % Inputs:
    %   jsonStr   - JSON string containing the configuration of bodies, joints, bushings,
    %               forces, solver parameters, and drives.
    %   designDic - (Optional) Structure containing a mapping dictionary to be applied
    %               to the JSON data for customized design configurations.
    %
    % Outputs:
    %   jointPrimStruct - Structure containing primary joint parameters.
    %   jointSecStruct  - Structure containing secondary joint parameters.
    %   bushPrimStruct  - Structure containing primary bushing parameters.
    %   bushSecStruct   - Structure containing secondary bushing parameters.
    %   forceAddStruct  - Structure containing additional force parameters.
    %   bodiesStruct    - Structure containing initialized bodies information.
    %   solverStruct    - Structure containing solver parameters.
    %   driveStruct     - Structure containing drive parameters.
    %   outputStruct    - Structure containing output parameters.
    % ##################################################################################

    % Decode JSON string into a structure
    jsonStruct = jsondecode(jsonStr);

    % Apply Design Mapping if designDic is provided, then validate JSON structure
    if ~isempty(designDic)
        jsonStruct = applydesignDicMapping(jsonStruct, designDic);
    end
    validateJsonStruct(jsonStruct);

    % Initialize Bodies defined by the user
    bodiesStruct = fun_initBodyStruct(jsonStruct.Bodies, jsonStruct.Hardpoints);

    % Initialize Joint Structures
    jointPrimStruct = fun_initJointStruct(jsonStruct.primary_joints, bodiesStruct, jsonStruct.drives.relonomic_drives);
    jointSecStruct  = fun_initJointStruct(jsonStruct.secondary_joints, bodiesStruct, jsonStruct.drives.relonomic_drives);

    % Create Joint Parameters
    jointPrimStruct = fun_createJointParam(jointPrimStruct);
    jointSecStruct  = fun_createJointParam(jointSecStruct);

    % Initialize Bushing Structures
    bushPrimStruct = fun_initBushStruct(jsonStruct.primary_bushings, bodiesStruct, jointPrimStruct);
    bushSecStruct  = fun_initBushStruct(jsonStruct.secondary_bushings, bodiesStruct, jointSecStruct);

    % Assign jointMode based on bushStruct 
    jointPrimStruct = setJointMode (jointPrimStruct, bushPrimStruct);
    jointSecStruct = setJointMode (jointSecStruct, bushSecStruct);

    % Initialize Force Additional Structures
    forceAddStruct = fun_initForceAddStruct(jsonStruct.additional_forces, bodiesStruct);

    % Create Bushing Parameters
    bushPrimStruct = fun_createConParam(bushPrimStruct);
    bushSecStruct  = fun_createConParam(bushSecStruct);

    % Create Force Additional Parameters
    forceAddStruct = fun_createConParam(forceAddStruct);

    % Initialize Solver Parameters
    solverStruct = fun_initSolverParameters(jointPrimStruct, jointSecStruct, bushPrimStruct, bushSecStruct, jsonStruct.sim_param.step_parameters);

    % Initialize Drive Parameters
    driveStruct = fun_initDriveStruct(jsonStruct, solverStruct);

    % Initalisation of Output paramters 
    outputStruct = initOutputStruct(jsonStruct.output_options);

end
