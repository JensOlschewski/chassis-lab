function eBasis = twoPtsToEbasis(P1, P2, axisString, helperVec, orthoAxisString)
    % Function: twoPtsToEBasis
    % Description: Computes an orthonormal basis given two points and two axis directions.
    % Inputs:
    %   P1, P2 - Two points defining the direction of one of the basis vectors.
    %   axisString - String ('X', 'Y', or 'Z') indicating which axis is defined by P1 and P2.
    %   helperVec - Helper vector used to determine the orthogonal directions.
    %   orthoAxisString - String ('X', 'Y', or 'Z') specifying the axis of the eBasis which is orthogonal to helperVec and defined by P1 and P2.
    % Output:
    %   eBasis - An orthonormal basis [ex; ey; ez] formed by the vectors ex, ey, and ez.

    % Helper function to normalize vectors
    normalize = @(v) v / norm(v);

    % Initialize empty basis vectors
    ex = [];
    ey = [];
    ez = [];

    % Compute the first vector based on axisString
    firstVec = normalize(P2 - P1);
    secondVec = cross(helperVec, firstVec);

    % Error check to ensure axis directions are different
    if strcmp(axisString, orthoAxisString)
        error("Axis for P1-P2 and orthogonal axis must be different");
    end

    % Assign the first basis vector
    switch axisString
        case 'X'
            ex = firstVec;
        case 'Y'
            ey = firstVec;
        case 'Z'
            ez = firstVec;
        otherwise
            error('Unknown axisString. Use <X>, <Y>, or <Z>');
    end

    % Assign the second basis vector
    switch orthoAxisString
        case 'X'
            ex = secondVec;
        case 'Y'
            ey = secondVec;
        case 'Z'
            ez = secondVec;
        otherwise
            error('Unknown orthoAxisString. Use <X>, <Y>, or <Z>');
    end

    % Compute the third basis vector to complete the right-handed system
    % Use the correct cross product based on assigned axes
    if isempty(ex)
        ex = cross(ey, ez);
    elseif isempty(ey)
        ey = cross(ez, ex);
    elseif isempty(ez)
        ez = cross(ex, ey);
    end

    % Normalize the basis vectors to ensure orthonormality
    ex = normalize (ex);
    ey = normalize (ey);
    ez = normalize (ez);

    % Assemble the basis
    eBasis = [ex, ey, ez];
end