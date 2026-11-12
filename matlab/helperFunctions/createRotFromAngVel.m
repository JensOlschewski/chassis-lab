function R = createRotFromAngVel(omega, h, method)
    % ##################################################################################
    % Function: createRotFromAngVel
    % Description: Computes a rotation matrix from an angular velocity vector and a small 
    %              time duration. The function offers two methods: 'small' for a small-angle 
    %              approximation suitable for minor rotations, and 'rodrigues' which uses 
    %              Rodrigues' rotation formula for more accurate rotation calculations over 
    %              larger angles.
    %
    % Inputs:
    %   omega  - Angular velocity vector (3x1), representing rotation rates about the x, y, and z axes.
    %   h      - Time duration for the rotation (scalar).
    %   method - Calculation method, either 'small' for small-angle approximation or 'rodrigues' 
    %            for Rodrigues' rotation formula.
    %
    % Output:
    %   R      - Rotation matrix (3x3) computed based on the specified method.
    % ##################################################################################

    % Compute the rotation angle
    theta = norm(omega) * h;

    % Select the method using switch
    switch method
        case 'small'
            % Small-angle approximation
            omegaHat = createSkewSymMat(omega);
            R = eye(3) + omegaHat * h;

        case 'rodrigues'
            % Rodrigues' rotation formula
            if theta < 1e-10
                R = eye(3);  % No rotation for very small angles
            else
                % Unit vector of the rotation axis
                omegaHat = omega / norm(omega);
                % Skew-symmetric matrix of the unit rotation axis
                K = createSkewSymMat(omegaHat);
                % Rodrigues' formula
                R = eye(3) + sin(theta) * K + (1 - cos(theta)) * (K * K);
            end

        otherwise
            error('Invalid method. Choose either "small" or "rodrigues".');
    end
end