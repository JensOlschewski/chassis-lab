function globalMatrix = lokMatToGlobal(localMatrix, eBasis)
    % ##################################################################################
    % Transforms a 6x6 matrix from the local eBasis coordinate system to the global coordinate system
    % Inputs:
    %   localMatrix - 6x6 local matrix
    %   eBasis - 3x3 matrix representing the local eBasis coordinate system
    %
    % Outputs:
    %   globalMatrix - 6x6 global stiffness or compliance matrix
    %
    % Description:
    %   The function creates a 6x6 block diagonal rotation matrix and 
    %   multiplies it with the local matrix to obtain the global matrix.
    % ##################################################################################

    % Validate input dimensions
    if ~isequal(size(localMatrix), [6, 6]) || ~isequal(size(eBasis), [3, 3])
        error('Invalid input dimensions. Expected a 6x6 local matrix and a 3x3 rotation matrix.');
    end

    % Create a 6x6 block diagonal rotation matrix
    rotMatrix6x6 = createBlkDiag(eBasis, eBasis);

    % Calculate the global 6x6 rotated matrix
    globalMatrix = rotMatrix6x6 * localMatrix * rotMatrix6x6';

end
