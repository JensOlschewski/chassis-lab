
function mat = createSkewSymMat(vec)
    % ##################################################################################
    % Function: createSkewSymMat
    %
    % Description:
    %   Constructs a skew-symmetric matrix from a 3-element vector.
    %
    % Inputs:
    %   vec - 3-element vector [x, y, z].
    %
    % Output:
    %   mat - 3x3 skew-symmetric matrix.
    % ##################################################################################
    % Validate input vector
    if numel(vec) ~= 3
        error('Input vector must have exactly 3 elements.');
    end
    
    % Extract elements from the input vector
    x = vec(1);
    y = vec(2);
    z = vec(3);
    
    % Construct the skew-symmetric matrix
    mat = [ 0, -z,  y;
            z,  0, -x;
           -y,  x,  0];
end