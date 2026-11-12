
function [T,T_inv] = createJointTransMat(s_skew)
    % ##################################################################################
    % Function: createJointTransMat
    %
    % Description:
    %   Calculates the transformation matrix from the joint point P to arbitrary  
    %   point D with the vector -s based on its type.
    %
    % Inputs:
    %   s_skew - Skew-symmetric matrix.
    %   type   - String specifying the joint type ('Reactionforce' or 'Velocity').
    %
    % Output:
    %   T      - Transformation matrix.
    % ##################################################################################
    % Define identity matrix
    E = eye(3);
    
    % Calculate transformation matrix (velocity) 
    T = [E, s_skew; 
        zeros(3), E];
    % Calculate inverse transformation matrix (force)
    T_inv = [E, -s_skew ;
            zeros(3),E];
end