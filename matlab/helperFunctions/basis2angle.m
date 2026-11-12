
% ##################################################################################
% basis2angle computes the projection angles between two orthonormal bases
% represented by iBasis and jBasis, in the coordinate system of jBasis.
%
% Inputs:
%   iBasis - The first orthonormal basis (3x3 matrix)
%   jBasis - The second orthonormal basis (3x3 matrix)
%
% Outputs:
%   ax - The angle between the x-axes of the two bases (in degrees, in jBasis)
%   by - The angle between the y-axes of the two bases (in degrees, in jBasis)
%   cz - The angle between the z-axes of the two bases (in degrees, in jBasis)
% ##################################################################################

function angles = basis2angle(iBasis, jBasis, mode)
    
    switch mode
        case "byrant"

            % Compute tranformation matrix T from j to i Basis
            T = jBasis' * iBasis;

            % Extract Euler angles (XYZ sequence) from transformation matrix T
            beta = asin(T(1,3));                % 
            gamma = atan2(-T(1,2), T(1,1));     %
            alpha = atan2(-T(2,3), T(3,3));     %
                        
            angles = [alpha; beta; gamma];

        case "projected"

            % Compute rotation matrix R from i to j
            T = jBasis' * iBasis;

            % Projections on yz, xz, and xy planes
            proj_alpha = atan2(-T(2,3), T(3,3)); % Projection of alpha on yz-plane (same as alpha)
            proj_beta  = atan2(T(1,3), T(3,3));  % Projection of beta on xz-plane
            proj_gamma = atan2(T(2,1), T(1,1));  % Projection of gamma on xy-plane

            angles = [proj_alpha; proj_beta; proj_gamma];

        otherwise
            error('Invalid mode. Choose either "byrant" or "projected"');
    end
end


