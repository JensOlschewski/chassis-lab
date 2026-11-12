function cMatrix = computeLocCompMat(kMatrix)
    % Computes the local compliance matrix from the local stiffness matrix
    % using vectorized operations for improved efficiency.
    %
    % Inputs:
    %   kMatrix - 6x6 local stiffness matrix
    %
    % Outputs:
    %   cMatrix - 6x6 local compliance matrix
    %
    % Description:
    %   This function calculates the local compliance matrix given a local stiffness matrix.
    %   The compliance matrix is computed by taking the reciprocal of each non-zero diagonal element
    %   in the stiffness matrix. If the diagonal element is zero, the corresponding compliance value is set to zero.
    %   This vectorized version enhances performance by avoiding explicit loops.

    % Validate input dimensions
    if ~isequal(size(kMatrix), [6, 6])
        error('Invalid input dimensions. Expected a 6x6 stiffness matrix.');
    end

    % Initialize the compliance matrix with zeros
    cMatrix = zeros(size(kMatrix));

    % Extract diagonal elements from the stiffness matrix
    diagElements = diag(kMatrix);

    % Identify non-zero diagonal elements
    nonZeroIndices = diagElements ~= 0;

    % Compute compliance values for non-zero diagonal elements and maintain zeros
    cMatrix(1:7:end) = 1 ./ (diagElements + ~nonZeroIndices) .* nonZeroIndices;
end
