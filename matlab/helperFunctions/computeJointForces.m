function [fpi,fpj,fsi,fsj] = computeJointForces(Gpi,Gpj,Gsi,Gsj,Tpfi,Tpfj,Tsfi,Tsfj,lambda)
    % computeJointForces computes forces in primary and secondary joints of a mechanical system
    %
    % This function calculates the forces in primary and secondary joints
    % based on constraint matrices, transformation matrices, and Lagrange multipliers.
    % Forces for primary and secondary joints are calculated separately and returned in four vectors.
    % The division of the Lagrange multipliers vector lambda is based on the number of
    % primary constraints.
    %
    % Inputs:
    %   Gpi, Gpj - Constraint matrices of primary joints
    %   Gsi, Gsj - Constraint matrices of secondary joints
    %   Tpfi, Tpfj - Transformation matrices of primary joints
    %   Tsfi, Tsfj - Transformation matrices of secondary joints
    %   lambda - Vector of Lagrange multipliers
    %
    % Outputs:
    %   fpi, fpj - Calculated force vectors for primary joints
    %   fsi, fsj - Calculated force vectors for secondary joints
    %
    % The function first determines the number of primary constraints and then
    % splits the vector of Lagrange multipliers accordingly to compute forces
    % for primary and secondary joints.

    numPrimconstraint = size(Gpi,1);

    % Compute primary joint forces
    fpi = Tpfi * Gpi' * lambda(1:numPrimconstraint);
    fpj = Tpfj * Gpj' * lambda(1:numPrimconstraint);

    % Compute secondary joint forces
    fsi = Tsfi * Gsi' * lambda(numPrimconstraint+1:end);
    fsj = Tsfj * Gsj' * lambda(numPrimconstraint+1:end);
end