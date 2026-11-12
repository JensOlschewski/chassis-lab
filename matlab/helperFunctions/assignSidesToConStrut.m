function [i_side,j_side] = assignSidesToConStrut(conStruct,bodiesStruct)
    % assignSidesToConStrut assigns the sides (left or right) for the 'i-part' and 'j-part'
    % based he side information from bodiesStruct. 
    %
    % For ground ID -> 0 it assign the same side as the counterpart i or j
    %
    % Inputs:
    %  - conStruct: Structure representing the connector, with fields 'i' and 'j' specifying the connecting bodies.
    %  - bodiesStruct: A structure array representing the bodies in the system, each with a 'side' field indicating its side.
    %
    % Output:
    %   - updatedjsonStruct : The updated JSON structure
    %
    % Error:
    %  - Throws an error if both 'i' and 'j' parts of the connector are connected to the ground (both have body ID 0).

    i_bodyID = conStruct.i.body;
    j_bodyID = conStruct.j.body;

    if i_bodyID == 0 && j_bodyID ~= 0
        j_side = bodiesStruct(j_bodyID).side; 
        i_side =j_side;

    elseif i_bodyID ~= 0 && j_bodyID == 0 
        i_side = bodiesStruct(i_bodyID).side; 
        j_side =i_side;

    elseif i_bodyID ~= 0 && j_bodyID ~= 0 
        i_side = bodiesStruct(i_bodyID).side; 
        j_side = bodiesStruct(j_bodyID).side;

    elseif i_bodyID == 0 && j_bodyID == 0  
        error("Both Joint sides i and j are connected to the ground");
    end 
end