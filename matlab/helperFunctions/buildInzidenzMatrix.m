function [S, W] = buildInzidenzMatrix(jointStruct)
    % buildInzidenzMatrix - Constructs an incidence matrix for a mechanical system.
    %
    % This function creates an incidence matrix based on the structure of joints 
    % connecting various bodies in a mechanical system. The function also calculates 
    % the inverse of this matrix.
    %
    % Inputs:
    %   jointStruct - A structure array where each element represents a joint in the 
    %                 mechanical system. Each element should have at least two fields, 
    %                 'i' and 'j', each containing a field 'bodyID' which refers to 
    %                 the IDs of the bodies connected by the joint.
    %
    % Outputs:
    %   S - Incidence matrix, a square matrix where the size is equal to the number 
    %       of bodies. The diagonal is filled with ones, and -1s indicate connections 
    %       between bodies.
    %   W - Inverse of the incidence matrix S.
    %
    % Example:
    %   jointStruct = struct('i', {struct('bodyID', 1)}, 'j', {struct('bodyID', 2)});
    %   [S, W] = buildInzidenzMatrix(jointStruct);
    
    % Determine the number of bodies
    numBodies = length(jointStruct);

    % Initialize the incidence matrix S
    S = zeros(numBodies);

    % Fill the main diagonal with ones
    S(1:numBodies+1:end) = 1;

    % Iterate through all joints to fill the matrix
    for i = 1:numBodies
        % bodyIDs of the current joint
        i_bodyID = jointStruct(i).i.bodyID;
        j_bodyID = jointStruct(i).j.bodyID;
                    
        if i_bodyID == 0 
            continue
        else
            % Case if i_bodyID and j_bodyID is reversed
            if i_bodyID > j_bodyID
                % Set -1 for the predecessor body
                S(i_bodyID,j_bodyID) = -1;
            else
                % Set -1 for the predecessor body
                S(j_bodyID,i_bodyID) = -1;
            end
        end
    end
    % Calculate the inverse of the incidence matrix
    W = inv(S);
end
