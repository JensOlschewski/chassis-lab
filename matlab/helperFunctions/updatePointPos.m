function newPoint = updatePointPos(oldPoint, oldCM, newCM, R)
    % Updates a point's position based on the new center of mass and a rotation matrix
    %
    % Inputs:
    %   oldPoint - Original point position
    %   oldCM - Old center of mass
    %   newCM - New center of mass
    %   R - Rotation matrix
    %
    % Output:
    %   newPoint - Updated point position
    %
    % Description:
    %   This function computes the new position of a point after a change in the center of mass and an applied rotation.
    %   It adjusts the point's position relative to the old center of mass, applies the rotation, and then shifts it 
    %   according to the new center of mass.

    % Compute the shift relative to the old center of mass
    s = oldPoint - oldCM; 

    % Apply rotation and shift to the new center of mass
    newPoint = newCM + R * s; 
end
