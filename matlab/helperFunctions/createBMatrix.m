function [B,Binv] = createBMatrix(joint_prim)
%CREATEBMATRIX Generates a matrix B and its inverse based on joint parameters.
%
%   This function takes a structure array representing joints in a robotic
%   system and constructs a matrix B. Each joint influences the structure
%   of B. The function then calculates the inverse of B, applying a
%   tolerance threshold to treat very small values as zero.
%
%   Inputs:
%       joint_prim - A structure array where each element represents a joint.
%                    Expected to contain fields 'i.bodyID', 'j.bodyID', and 'Bji'.
%
%   Outputs:
%       B    - A square matrix constructed from joint parameters.
%       Binv - The inverse of the matrix B, with very small values set to zero.

    k = length(joint_prim);  % Number of joints
    B = eye(6*k);            % Initialize the B identity matrix
    
    % Populate other blocks based on bodyIDs
    for i = 1:k
        i_bodyID = joint_prim(i).i.bodyID;
        j_bodyID = joint_prim(i).j.bodyID;

        % Skip if i_bodyID or j_bodyID is zero
        if i_bodyID == 0 || j_bodyID == 0
            continue;
        end

        Bji = joint_prim(i).Bji;

        % Calculate indices for the block entry
        rowIndex = 6*(j_bodyID-1) + (1:6);
        colIndex = 6*(i_bodyID-1) + (1:6);

        % Check if j_bodyID > i_bodyID 
        if i_bodyID > j_bodyID
            B(colIndex, rowIndex) = -Bji;
        else
            % Fill the corresponding block
            B(rowIndex, colIndex) = -Bji;
        end
    end

    % Invert B matrix 
    Binv = inv(B);
    
    % Set small values to zero
    tolerance = 1e-10;  % Define a tolerance limit
    Binv(abs(Binv) < tolerance) = 0;
end
    