% #########
%  Helper function to createConDistanceVector
% #########
% Function: createConDistanceVector
%
% This function calculates the distance vector s for a 
% connector part relative to the center of mass of the associated body. 
% It is primarily used in multibody system simulations to determine the r
% elative position of a joint or bushing on a body.
%
% Inputs:
%  - conPart: A structure representing the connection part, containing fields 'bodyID' and 'pointName'.
%    'bodyID' is an identifier for the body to which the connection part is attached. 
%    'pointName' is the name of the point on the body where the connection is made.
%  - bodyStruct: A structure array representing the bodies in the system. Each element of the array contains 
%    a 'cm' field representing the center of mass, and a 'points' field, which is 
%    a structure mapping point names to their coordinates.
%
% Outputs:
%  - s: A 3-element column vector representing the distance from the body's center of mass to the connection point. 
%    If the 'bodyID' is 0 (indicating connection to the ground), the function returns a zero vector.
%
% Example:
% distanceVector = createConDistanceVector(conPart, bodyStruct);

function s = createConDistanceVector(bodyStruct,conPart)
    bodyIdentifier=conPart.bodyID;
    pointname = conPart.pointName;
    if bodyIdentifier == 0 
        s = zeros(3,1);
    else
        bodyStruct = bodyStruct(bodyIdentifier);
        s = bodyStruct.points.(pointname) - bodyStruct.cm;
    end
end