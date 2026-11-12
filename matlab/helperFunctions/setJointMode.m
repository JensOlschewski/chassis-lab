function jointStruct = setJointMode(jointStruct, bushStruct)
    % ##################################################################################
    % Function: setJointMode
    % Description: Sets the jointMode of jointStruct based on the active status
    %              in bushStruct. Assigns 'elastic' if active is true, otherwise 'kinematic'.
    %
    % Inputs:
    %   jointStruct - Structure array to be updated with joint modes.
    %   bushStruct  - Structure array containing the active status.
    %
    % Output:
    %   jointStruct - Updated structure array with assigned joint modes.
    % ##################################################################################
        
    % Initialize jointMode to 'kinematic'
    for k = 1:numel(jointStruct)
        jointStruct(k).jointMode = 'kinematic';
    end

    % Loop through each element of bushStruct
    for i = 1:length(bushStruct)
        % Extract the jointID 
        jointID = bushStruct(i).jointID;
    
        if bushStruct(i).active
            jointStruct(jointID).jointMode = 'elastic';
        end
    end
end
