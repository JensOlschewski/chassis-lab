function eBasis = threePtsToEbasis(P1, P2, P3, axisP12, axisP13)
    % ##################################################################################
    % Function: threePtsToEbasis
    % Description: Computes an orthonormal basis given three points and two axis directions.
    % Inputs:
    %   P1, P2, P3 - Points defining the directions of the basis vectors.
    %   axisP12 - String ('X', 'Y', or 'Z') indicating the axis direction defined by P1 and P2.
    %   axisP13 - String ('X', 'Y', or 'Z') indicating the axis direction defined by P1 and P3.
    % Output:
    %   eBasis - An orthonormal basis [ex; ey; ez] formed by the vectors ex, ey, and ez.
    % ##################################################################################
    
    % Helper function to normalize vectors
    normalize = @(v) v / norm(v);

    % Initialize empty basis vectors
    ex = [];
    ey = [];
    ez = [];

    % Check that axisP12 and axisP13 are different
    if strcmp(axisP12, axisP13)
        error("Axis for P1-P2 and P1-P3 must be different");
    end

    % Compute direction vectors
    vecP12 = (P2 - P1);
    vecP13 = (P3 - P1);
    

    % Assign the first basis vector based on axisP12
    switch axisP12
        case "X"
            ex = vecP12;
        case "Y"
            ey = vecP12;
        case "Z"
            ez = vecP12;
        otherwise
            error("Unknown axis for P1 and P2, define <X>, <Y>, or <Z>");
    end

    % Assign the second basis vector based on axisP13
    switch axisP13
        case "X"
            ex = vecP13;
        case "Y"
            ey = vecP13;
        case "Z"
            ez = vecP13;
        otherwise
            error("Unknown axis for P1 and P3, define <X>, <Y>, or <Z>");
    end

    % Compute the third basis vector to complete the right-handed system
    if isempty(ex)
        ex = (cross(ey, ez));
    elseif isempty(ey)
        ey = (cross(ez, ex));
    elseif isempty(ez)
        ez = (cross(ex, ey));
    end
    
    % Normalize the basis vectors to ensure orthonormality
    ex = normalize (ex);
    ey = normalize (ey);
    ez = normalize (ez);

    % Assemble the basis
    eBasis = [ex ey ez];
end