% Function: fun_initJointStruct
% Description: Initializes and structures joint data for a mechanical system.
% Inputs:
%   jsonJointStruct - Input structure containing initial joint information from JSON.
%   bodiesStruct - Structure containing information about the bodies involved in the joints.
%   relonomicDriveStruct - Structure containing information about the relonomic drives.
% Output:
%   jointStructOut - Output structure with organized and additional joint information.

function jointStructOut = fun_initJointStruct(jsonJointStruct, bodiesStruct, relonomicDriveStruct)
    % Extract the names of the joints from the input structure.
    jointNames = fieldnames(jsonJointStruct);
    
    % Initialize an empty structure for output.
    jointStructOut = struct();

    % Iterate over each joint to reorganize and enhance the joint information.
    for i = 1:length(jointNames)
        % Retrieve the currect joint name
        jointName = jointNames{i};
        
        % Retrieve the current joint ID
        jointID = jsonJointStruct.(jointName).jointID;  

        % Assign basic joint properties (name, ID, type, topology) to the output structure.
        jointStructOut(jointID).name = jointName;
        jointStructOut(jointID).jointID = jointID;
        jointStructOut(jointID).type = jsonJointStruct.(jointName).type;
        jointStructOut(jointID).conType = 'joint';
        jointStructOut(jointID).topology = jsonJointStruct.(jointName).jointTopology;
        
        % Determine the sides of the constraint structure associated with the joint.
        [i_side, j_side] = assignSidesToConStrut(jsonJointStruct.(jointName), bodiesStruct);
        
        % Create constraint parts for both sides of the joint.
        jointStructOut(jointID).i = createConPart(jsonJointStruct.(jointName).i, bodiesStruct, i_side); 
        jointStructOut(jointID).j = createConPart(jsonJointStruct.(jointName).j, bodiesStruct, j_side);
        
        % Check if jBody > iBody 
        jointStructOut(i).isJGreaterThanI = jointStructOut(i).i.bodyID < jointStructOut(i).j.bodyID;

        % Generate a logical joint constraint matrix based on the joint type.
        jointStructOut(jointID).eG = createLogicalJointConstMatix(jsonJointStruct.(jointName).type);
    end

    % Incorporate relonomic drive constraints into the joint structure.
    jointStructOut = createRelonomicDriveConstrCols(relonomicDriveStruct, jointStructOut);
end
