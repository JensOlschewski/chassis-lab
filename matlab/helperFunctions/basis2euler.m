function [phi, theta, psi] = basis2euler(eBasis,uMode)
    % ##################################################################################
    % BASIS2EULER computes the Euler angles (phi, theta, psi) from a given
    % rotation matrix 'eBasis' using the specified Euler angle convention.
    %
    % Inputs:
    %   eBasis - A 3x3 rotation matrix representing the change of basis from
    %            the reference coordinate system to the target coordinate system.
    % Outputs:
    %   phi   - Rotation angle around the first axis (in degrees).
    %   theta - Rotation angle around the second axis (in degrees).
    %   psi   - Rotation angle around the third axis (in degrees).
    % ##################################################################################

    % eBasis defined in global coordinate-system -> rotation matrix 
    R = eBasis;

    switch lower(uMode)

        case 'xyz'
            % -- XYZ Convention --
            % Compute theta
            theta = atan2(-R(3,1), sqrt(R(3,2)^2 + R(3,3)^2));
            % Compute phi
            if cos(theta) == 0
                phi = atan2(R(2,3), R(2,2));
            else
                phi = atan2(R(3,2)/cos(theta), R(3,3)/cos(theta));
            end
            %Compute psi
            if cos(theta) == 0
                psi = 0; % Undefined, can set to 0
            else
                psi = atan2(R(2,1)/cos(theta), R(1,1)/cos(theta));
            end

        case 'zxz'
            % -- ZXZ Convention --
            if abs(R(3,3)) ~= 1
                theta = acos(R(3,3));  
                psi = atan2(R(3,1), R(3,2));
                phi = atan2(R(1,3), -R(2,3));


            else
                % Check for gimbal lock
                theta = 0;
                phi = 0;
                if R(3,3) == -1
                    psi = atan2(R(1,2), R(1,1));
                else
                    psi = atan2(-R(1,2), -R(1,1));
                end
            end
    
        otherwise
            error('Invalid uMode. Use ''xyz'' or ''zxz''.');
            
    end
    % Convert angles from radians to degrees if necessary
    phi = rad2deg(phi);
    theta = rad2deg(theta);
    psi = rad2deg(psi);

end
