function eBasis = deformationToEBasis(kMatrix, euler, forceConstraint, defDirString, helperVec, axisOrtho)
    % ##################################################################################
    % deformationToEBasis
    % Description: Computes the deformation direction based and converts it to an orthonormal basis.
    % Inputs:
    %   kMatrix - Stiffness matrix [k1...k6]
    %   euler - Euler angles [eulerAngleArray]
    %   forceConstraint - Force constraint vector [unitVector]
    %   defDirString - Deformation direction string ('X', 'Y', or 'Z')
    %   helperVec - Helper vector [unitVector]
    % Output:
    %   eBasis - An orthonormal basis [ex; ey; ez] formed by the vectors ex, ey, and ez.
    % ##################################################################################

    % Convert stiffness matrix to compliance matrix
    cMatrixLok = computeLocCompMat(diag(kMatrix));

    % Compute rotation matrix from Euler angles
    eBasisCMat = eulerToRotation(euler);

    % Transform local compliance matrix to global compliance matrix
    cMatrix = lokMatToGlobal(cMatrixLok, eBasisCMat);

    % Compute deformation direction using the upper left 3x3 block of cMatrix
    cMatrix3x3 = cMatrix(1:3, 1:3);
    deformationDirection = cMatrix3x3 * forceConstraint;

    % Determine the two remaining basis vectors
    eBasis = twoPtsToEbasis([0, 0, 0]', deformationDirection, defDirString, helperVec, axisOrtho);
end
