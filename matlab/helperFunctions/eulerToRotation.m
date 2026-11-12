
function R = eulerToRotation(eulerAngleArray)
    % ##################################################################################
    % Function: eulerToRotation
    %
    % Description:
    %   Converts Euler angles to a rotation matrix based on the specified convention.
    %
    % Inputs:
    %   eulerAngleArray - A 1x3 array containing Euler angles [phi, theta, psi] in degrees.
    %   Umode           - String specifying the Euler angle convention ('xyz' or 'zxz').
    %
    % Outputs:
    %   R - A 3x3 rotation matrix derived from the given Euler angles.
    %
    % Example:
    %   R = eulerToRotation([45, 30, 60], 'xyz');
    % ##################################################################################
    
    % Extract Euler angles from array
    phi = eulerAngleArray(1);
    theta = eulerAngleArray(2);
    psi = eulerAngleArray(3);

    RzPhi = rotationZ(phi);
    RxTheta = rotationX(theta);
    RzPsi = rotationZ(psi);

    R = RzPhi * RxTheta * RzPsi;
    R = applyTolerance(R, 1e-8);
end

% #########
%  Helper function for Z-rotation
% #########;

function R = rotationZ(angle_deg)
    R = [cosd(angle_deg), -sind(angle_deg), 0;
         sind(angle_deg),  cosd(angle_deg), 0;
         0,               0,              1];
end

% #########
%  Helper function for X-rotation
% #########

function R = rotationX(angle_deg)
    R = [1, 0,                0;
         0, cosd(angle_deg), -sind(angle_deg);
         0, sind(angle_deg),  cosd(angle_deg)];
end

