function [ebasis,startPoint,endPoint,startPointName,endPointName,isGroundStart,isGroundEnd] = orientConnByTwoPts(connectorStruct, bodiesStruct, bodyID, part_side)
    % ORIENTCONNBYTWOPTS Creates a structure for connection points
    % This function takes the connector structure, bodies structure, body ID, 
    % and part side to generate a connection structure. It calculates the start 
    % and end points of a connector based on the given parameters and computes
    % the rotation matrix.
    %
    % Inputs:
    %   connectorStruct - A structure containing connector's orientation and axis
    %   bodiesStruct - A structure array containing body points
    %   bodyID - An integer representing the ID of the body
    %   part_side - A string or numeric value representing the side of the part
    %
    % Outputs:
    %   connStruct - A structure containing the calculated connection information
    %
    % Example usage:
    %   connStruct = orientConnByTwoPts(connectorStruct, bodiesStruct, 3, 'left');

    % Retrieve original start and end point names
    originalstartPointName = connectorStruct.orientation.startpoint;  
    originalendPointName = connectorStruct.orientation.endpoint; 

    % Initialize indexes and boolean flags
    [bodyID_start_index, bodyID_end_index, isGroundStart, isGroundEnd] = initializeIndexesAndFlags(bodyID, bodiesStruct, originalstartPointName, originalendPointName, part_side);

    % Generate point names
    startPointName = generatePointName(originalstartPointName, bodyID, part_side, isGroundStart);
    endPointName = generatePointName(originalendPointName, bodyID, part_side, isGroundEnd);

    % Assign start and end points
    startPoint = bodiesStruct(bodyID_start_index).points.(startPointName);
    endPoint = bodiesStruct(bodyID_end_index).points.(endPointName);

    % Calculate rotation matrix and assign connection structure
    axis = connectorStruct.orientation.axis; 
    ebasis = twoPtsToEbasis(startPoint, endPoint, axis, 'X');

end

function [start_index, end_index, isStart, isEnd] = initializeIndexesAndFlags(bodyID, bodiesStruct, startPointName, endPointName, part_side)
    % Helper function to initialize indexes and boolean flags

    % Check existence of start and end points and assign flags and indexes
    if bodyID ~= 0 
        [start_index, end_index] = deal(bodyID);
        isStart = ~checkPointExistence(bodiesStruct(bodyID).points, generatePointName(startPointName, bodyID, part_side, false));
        isEnd = ~checkPointExistence(bodiesStruct(bodyID).points, generatePointName(endPointName, bodyID, part_side, false));
        if isStart
            start_index = length(bodiesStruct);
        end
        if isEnd
            end_index = length(bodiesStruct);
        end
    else
        start_index = length(bodiesStruct); 
        end_index  = length(bodiesStruct);
        isStart = true; 
        isEnd = true;
    end
end

% Helper function to check point existence
function isPointExist = checkPointExistence(bodiesStruct, pointName)
    isPointExist = isfield(bodiesStruct, pointName);
end
