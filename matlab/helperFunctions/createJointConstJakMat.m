% #########
%  Helper function for explicit and implicit joint matrices 
% #########
%
% This function creates the constraint Jacobian matrices (Gi, Gj, Hj) for different types of joints in a multibody system. 
% It calculates these matrices based on the joint type, Euler basis vectors, and positional offsets of the joint parts.
%
% Inputs:
%  - i_ebasis, j_ebasis: Euler basis vectors for the 'i' and 'j' parts of the joint.
%  - si, sj: Positional offsets for the 'i' and 'j' parts.
%  - i_point, j_point: Coordinates of the 'i' and 'j' points of the joint.
%
% Outputs:
%  - Gi, Gj, Hj: Jacobian matrices representing the constraints imposed by the joint on the 'i' and 'j' parts.
%
% Supported joint types include 'Drehgelenk', 'Kugelgelenk', 'Kardangelenk', 'Ebenengelenk', 'KugelSchubgelenk', 'Schubfuehrung', and 'Symmetriebedingung'.
%
% Example:
% [Gi, Gj, Hj] = createJointConstJakMat(i_ebasis, j_ebasis, si, sj, i_point, j_point);



function [Gi,Gj,Hj] = createJointConstJakMat(i_ebasis,j_ebasis,si,sj,i_point,j_point,jointType)
    %
    ei_1 = i_ebasis(:,1); 
    ei_2 = i_ebasis(:,2);
    ei_3 = i_ebasis(:,3);
    %
    %ej_1 = j_ebasis(:,1); 
    ej_2 = j_ebasis(:,2);
    ej_3 = j_ebasis(:,3);
    %
    si_skew = createSkewSymMat(si);
    sj_skew = createSkewSymMat(sj);
    %         
    l_skew = createSkewSymMat(j_point-i_point);
    %
    dj = -sj;
    dj_skew = -sj_skew;
    E = eye(3);
    Z = zeros(1,3);
            
    switch jointType

        case 'Drehgelenk'  % Revolute Joint
            % -> ei_1 and ei_2 perpendicular to revolute axis (i-Part)
            % -> ej_3 parallel to revolute axis (j-Part)

            % Caclulate ej_skew
            ej_3_skew = createSkewSymMat(ej_3);

            % Calculate Gi, Gj, and Hj
            Gi = [-E, si_skew; Z, ei_1' * ej_3_skew; Z, ei_2' * ej_3_skew];
            Gj = [E, -sj_skew; Z, -ei_1' * ej_3_skew; Z, -ei_2' * ej_3_skew];
            Hj = [ej_3_skew * dj; ej_3];
            
        case 'Kugelgelenk'  % Ball Joint
            % -> no direction needs to be defined (i-Part)
            % -> no direction needs to be defined (j-Part)

            % Calculate Gi, Gj, and Hj
            Gi = [-E, si_skew];
            Gj = [E, -sj_skew];
            Hj = [-dj_skew;E];
            
        case 'Kardangelenk'  % Universal Joint
            % -> ei_3 parallel to first axis (i-Part)
            % -> ej_2 parallel to second axis (j-Part)

            % Caclulate ei_skew from ej_2
            ei_3_skew = createSkewSymMat(ei_3);
            % Caclulate ej_skew from ei_3
            ej_2_skew = createSkewSymMat(ej_2);

            % Calculate Gi, Gj, and Hj
            Gi = [-E, si_skew; Z, ei_3' * ej_2_skew];
            Gj = [E, -sj_skew; Z, -ei_3' * ej_2_skew];
            Hj = [ei_3_skew * dj,ej_2_skew * dj;ei_3,ej_2];

        case 'Ebenengelenk'  % Planar Joint
            % -> ei_1 and ei_2 defines plane (i-Part)
            % -> ej_3 parallel rotation axis (j-Part)

            % Caclulate ej_skew
            ej_3_skew = createSkewSymMat(ej_3);
                
            % Calculate Gi, Gj, and Hj
            Gi = [-ej_3',ej_3'*(si_skew+l_skew);Z,ei_1'*ej_3_skew;Z,ei_2'*ej_3_skew];
            Gj = [ej_3',-ej_3'*sj_skew;Z,-ei_1'*ej_3_skew;Z,-ei_2'*ej_3_skew];            
            Hj = [ei_1,ei_2,ej_3_skew*dj;zeros(3,1),zeros(3,1),ej_3];

        case 'KugelSchubgelenk'  % Inline-Joint
            % -> ei_1, ei_2 and ei_3 defines the cylindircal part (i-Part)
            % -> ej_3 parallel to translation axis (j-Part)

            Gi = [-ei_1',ei_1'*(si_skew+l_skew);-ei_2',ei_2'*(si_skew+l_skew)];
            Gj = [ei_1',-ei_1'*sj_skew;ei_2',-ei_2'*sj_skew];
            Hj = [-dj_skew,ei_3,E,zeros(3,1)];
            
        case 'Schubfuehrung'  % Prismatic Guide
            % -> ei_1, ei_2 defines prismatic part (i-Part)
            % -> ej_3 defines prismatic guide direction (j-Part)

            Gi = [-ei_1', ei_1'*(si_skew+l_skew);-ei_2',ei_2'*(si_skew+l_skew);zeros(3,3),E];
            Gj = [ei_1', -ei_1'*sj_skew;ei_2',-ei_2'*sj_skew;zeros(3,3),-E];
            Hj = [ej_3;zeros(3,1)];

        case 'Symmetriebedingung'  % Symmetry Constraint
            % -> ei_3 defines the symmetry direction 
            Gi = [ei_3',-ei_3'*si_skew];
            Gj = [-ej_3',ej_3'*sj_skew];
            Hj = NaN;

        otherwise
            error('Unsupported joint type');
    end
end
