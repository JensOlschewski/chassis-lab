% #########
%  Helper function to create rji Vector and Bji matrix
% #########
% This function computes the relative position vector (rji) and the corresponding transformation matrix (Bji) 
% between two points, typically used in multibody dynamics for calculating offsets between center of masses of bodies.
%
% Inputs:
%  - cmj, cmi: Vectors representing the center of mass of two bodies (j and i).
%
% Outputs:
%  - rji: The relative position vector from cmj to cmi.
%  - Bji: A 6x6 transformation matrix associated with the offset.
%
% The transformation matrix Bji includes a skew-symmetric matrix based on rji and is used in joint and force calculations.
%
% Example:
% [rji, Bji] = createOffsetParam(cmj, cmi);
function [rji,Bji] = createOffsetParam(cmj,cmi)
    %
    rji = cmi - cmj; 
    %
    E = eye(3);
    r_skew = createSkewSymMat(rji); 
    Bji= [E, r_skew; zeros(3), E];
    %
end