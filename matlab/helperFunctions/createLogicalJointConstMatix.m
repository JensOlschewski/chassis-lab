% #########
%  Helper function to create ConstrainMatrix for each joint
% #########
% Function: createLogicalJointConstMatix
%
% This function creates a constraint matrix for a specified joint type in a multibody system. 
% The matrix defines the degrees of freedom (DOFs) that are constrained or allowed for the joint. 
% Each type of joint (e.g., universal joint, ball joint) has a specific pattern of constraints.
%
% Inputs:
%  - jointType: A string indicating the type of the joint. Supported joint types include:
%    'Kardangelenk' (universal joint), 'Kugelgelenk' (ball joint), 'Drehgelenk' (rotational joint),
%    'Schubfuehrung' (sliding joint), 'Ebenengelenk' (plane joint), 'KugelSchubgelenk' (ball and socket joint), 
%    and 'Symmetriebedingung' (symmetry condition).
%
% Outputs:
%  - jointConstraintMatrix: A matrix representing the constraints of the joint. 
%    Each row corresponds to a constrained DOF (tx, ty, tz, rx, ry, rz) for the joint.
%    The size of the matrix depends on the joint type and the number of constrained DOFs.
%
% Example:
% matrix = createLogicalJointConstMatix('Kugelgelenk');

function jointConstraintMatrix = createLogicalJointConstMatix(jointType)
    % Initialisiere die Matrix mit Nullen und definiere die ConstraintDirections
    dof = zeros(6, 1); % 6 ConstraintDirectionss: tx, ty, tz, rx, ry, rz
    % Setze die DOFs basierend auf dem Gelenktyp
    switch jointType
        case 'Kardangelenk'
            dof([1, 2, 3, 4]) = 1; % tx, ty, tz, rx
        case 'Kugelgelenk'
            dof([1, 2, 3]) = 1; % tx, ty, tz
        case 'Drehgelenk'
            dof([1, 2, 3, 4, 5]) = 1; % tx, ty, tz, rx, ry
        case 'Schubfuehrung'
            dof([1, 2, 4, 5, 6]) = 1; % tx, ty, rx, ry, rz
        case 'Ebenengelenk'
            dof([3, 4, 5]) = 1; % tz, rx, ry       
        case 'KugelSchubgelenk'
            dof([1, 2]) = 1; % tx, ty
        case 'Symmetriebedingung'
            dof([1, 2, 4, 5, 6]) = 1; % tz
        otherwise
            error('Unbekannter Gelenktyp: %s', jointType);
    end
    % Erstelle die Constraint-Matrix
    jointConstraintMatrix = zeros(sum(dof), 6); % Matrix mit dynamischer Anzahl an Zeilen, aber immer 6 Spalten
    jointConstraintMatrix(:, dof == 1) = eye(sum(dof));
end
