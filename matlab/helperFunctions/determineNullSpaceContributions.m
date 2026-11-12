
function [complianceRange, stiffnessNullSpace] = determineNullSpaceContributions(complianceMatrix, stiffnessMatrix, eG)
    % ##################################################################################
    % Function: determineNullSpaceContributions
    % Description: Determines the range and null space contributions from compliance 
    %              and stiffness matrices.
    %
    % Inputs:
    %   complianceMatrix - Compliance matrix (C)
    %   stiffnessMatrix - Stiffness matrix (K)
    %   eG - Matrix for which the range and null space are determined
    %
    % Outputs:
    %   complianceRange - Compliance matrix with columns set to zero if contributing 
    %                     to the null space of eG
    %   stiffnessNullSpace - Stiffness matrix with columns set to zero if contributing 
    %                        to the null space of eG
    %
    % Description:
    %   This function determines how the columns of the compliance and stiffness 
    %   matrices contribute to the range and null space of eG. For the compliance 
    %   matrix, columns in the null space of eG are set to zero, thus determining 
    %   the range. For the stiffness matrix, columns in the null space of eG are 
    %   set to zero.
    % ##################################################################################
    nullSpace = null(eG);

    % Projection Matrix 
    P = nullSpace * nullSpace';

    % Determine ComplianceRange
    complianceNullSpace = complianceMatrix * P;
    complianceRange = complianceMatrix-complianceNullSpace;

    % Stiffness in NullSpace
    stiffnessNullSpace = stiffnessMatrix * P;