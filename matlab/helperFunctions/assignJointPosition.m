function updatedConStruct = assignJointPosition(jointStruct, oldConStruct)
    % ##################################################################################
    % assignJointPosition Assigns joint positions to the bush structure.
    %
    % This function updates the bush structure with joint position data from the
    % given joint structure. It iterates over each bush, checks if it's active,
    % and updates the 'eBasisJoint', 'pointJoint' fields for each side ('i' and 'j').
    %
    % Inputs:
    %   jointStruct    - Structure containing joint parameters.
    %   oldConStruct   - Structure containing existing connector parameters.
    %
    % Outputs:
    %   updatedConStruct - Structure containing the updated bush parameters.
    %
    % ##################################################################################

    
    updatedConStruct = oldConStruct;
    for i = 1:numel(updatedConStruct)
        
        if  updatedConStruct(i).active == 0
            continue;   
        end

        jointID = updatedConStruct(i).jointID;
        conSides = {'i', 'j'};


        for k = 1:numel(conSides)
            conSide = conSides{k};

            if updatedConStruct(i).(conSide).bodyID == jointStruct(jointID).(conSide).bodyID
                % case: IDs match, use conSides{k}
                jointSide = conSide;
            else 
                % case: IDs differ, use the 'opposite' index
                jointSide = conSides{3-k};

            end

            updatedConStruct(i).(conSide).eBasisJoint= jointStruct(jointID).(jointSide).eBasis;
            updatedConStruct(i).(conSide).pointJoint= jointStruct(jointID).(jointSide).point;           
        end
    end
end
