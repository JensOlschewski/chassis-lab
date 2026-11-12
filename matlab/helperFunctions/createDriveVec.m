function driveStruct = createDriveVec (driveStructIn,signalStruct,numB)
    % createDriveVec constructs a vector from drive structures and signal gradients
    %
    % This function builds a drive vector for each body in the mechanical system
    % based on the gradients of signals provided in the driveStructIn. The function
    % iterates through each drive, updating segments of the drive vector corresponding
    % to each body, based on the signal gradients. The vector is constructed for a 
    % specified number of bodies and signal length.
    %
    % Inputs:
    %   driveStructIn - Structure containing information about each drive, including
    %                   bodies involved (i_body and j_body) and signal gradients
    %   signalStruct - Structure containing signal gradient data used in calculations
    %   numB - The total number of bodies in the mechanical system
    %
    % Output:
    %   driveVec - The constructed drive vector, with each segment corresponding to
    %              the influence of drive signals on each body

    % Signal length
    signal_length=size(signalStruct(1).signal_grad,2);

    % Initialize the output vector
    driveStruct.signalVec = zeros(6*numB,signal_length);
    driveStruct.driveVec = zeros(6*numB,signal_length);

    % Get fieldnames
    namesdriveStruct = fieldnames(driveStructIn);
    
    % Iterate through each entry in DriveStruct
    for i = 1:length(namesdriveStruct)
        
        %
        namedriveStruct = namesdriveStruct{i};

        i_body = driveStructIn.(namedriveStruct).i;
        j_body = driveStructIn.(namedriveStruct).j;

        % Update the segment for body i_body, if i_body is not zero
        if i_body ~= 0
            driveStruct.signalVec(i_body*6-5:i_body*6,:) = driveStruct.signalVec(i_body*6-5:i_body*6,:) - signalStruct(i).signal_array;
            driveStruct.driveVec(i_body*6-5:i_body*6,:) = driveStruct.driveVec(i_body*6-5:i_body*6,:) - signalStruct(i).signal_grad;
        end

        % Update the segment for body j_body, if j_body is not zero
        if j_body ~= 0
            driveStruct.signalVec(j_body*6-5:j_body*6,:) = driveStruct.signalVec(j_body*6-5:j_body*6,:) + signalStruct(i).signal_array;
            driveStruct.driveVec(j_body*6-5:j_body*6,:) = driveStruct.driveVec(j_body*6-5:j_body*6,:) + signalStruct(i).signal_grad;
        end
    end
end